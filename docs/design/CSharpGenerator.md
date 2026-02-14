# Design: C Header → WinMD Pipeline (Linux, No MIDL)

## Goal

Build a cross-platform pipeline (**bindscrape**) that takes arbitrary C/C++ header files
and produces an ECMA-335 `.winmd` file — without requiring Windows, Visual Studio, or
MIDL. The pipeline reuses the same core tools as
[microsoft/win32metadata](https://github.com/microsoft/win32metadata) but replaces
the Windows-only orchestration layer with a custom, Linux-compatible one.

## Key Insight

The win32metadata pipeline has **two separable concerns**:

1. **Tooling** — [ClangSharp](https://github.com/dotnet/ClangSharp) (libClang-based
   C parser → C# source) and the
   [Emitter](https://github.com/microsoft/win32metadata/tree/main/sources/ClangSharpSourceToWinmd)
   (C# source → ECMA-335 winmd). These are pure .NET and cross-platform.
2. **Orchestration** — The
   [WinmdGenerator MSBuild SDK](https://github.com/microsoft/win32metadata/tree/main/sources/GeneratorSdk),
   [PowerShell scripts](https://github.com/microsoft/win32metadata/blob/main/scripts/DoAll.ps1),
   Windows SDK NuGet packages, and MIDL. These are Windows-only and unnecessary for
   custom headers.

bindscrape replaces concern (2) while reusing concern (1).

---

## Architecture

```
User C/C++ Headers
       │
       ▼
┌──────────────┐
│  ClangSharp   │  .NET global tool (ClangSharpPInvokeGenerator)
│  PInvoke Gen  │  Uses libClang (LLVM) — works on Linux
└──────┬────────┘
       │  Generated C# source files (one per namespace/partition)
       ▼
┌──────────────┐
│  (Optional)   │  ConstantsScraper — regex-based #define extraction
│  Constants    │  Only needed if headers have #define constants
└──────┬────────┘
       │  Additional C# source for constants
       ▼
┌──────────────┐
│  Emitter      │  ClangSharpSourceCompilation + ClangSharpSourceWinmdGenerator
│               │  C# Roslyn compilation → ECMA-335 MetadataBuilder → .winmd
└──────┬────────┘
       │
       ▼
  output.winmd
```

---

## Stage 1: ClangSharp — C Headers → C# P/Invoke Source

### What It Does

[ClangSharpPInvokeGenerator](https://github.com/dotnet/ClangSharp#readme) is a .NET tool
that wraps LLVM's [libClang](https://clang.llvm.org/doxygen/group__CINTERFACE.html) to parse C/C++
headers and emit C# source files containing:

- **Structs** — mapped to C# `struct` with `[StructLayout]`
- **Functions** — mapped to `static extern` methods with `[DllImport]`
- **Enums** — mapped to C# `enum`
- **Typedefs** — mapped via `[NativeTypeName]` attributes or struct wrappers
- **COM interfaces** — mapped to C# `struct` with vtable delegates (later converted to
  proper interfaces by the Emitter)
- **Function pointers** — mapped to C# `delegate`
- **Constants** — only those that appear as `const` in the AST (not `#define`)

### Why It Works on Linux

ClangSharp uses [libClang](https://clang.llvm.org/doxygen/group__CINTERFACE.html), which
is the stable C API for the LLVM/Clang compiler. libClang is fully cross-platform. The
[ClangSharpPInvokeGenerator NuGet package](https://www.nuget.org/packages/ClangSharpPInvokeGenerator)
bundles native libClang binaries for Linux (x64, arm64), macOS, and Windows via the
[libClangSharp runtime packages](https://www.nuget.org/packages/libClangSharp).

### How to Invoke

Install as a .NET global tool:

```bash
dotnet tool install --global ClangSharpPInvokeGenerator
```

Invoke directly or via a response file (`.rsp`):

```bash
ClangSharpPInvokeGenerator @my_headers.rsp
```

### Response File Configuration

The response file controls all behavior. Key options for custom headers:

```rsp
# Input
--file
my_header.h

# Where to find includes
--include-directory
/usr/include
--include-directory
./vendor/include

# Output directory
--output
./generated/cs

# Namespace for generated types
--namespace
MyLibrary.Interop

# Only traverse (emit types from) these files, not all transitive includes
--traverse
my_header.h

# Language mode
-x
c
# or: -x c++

# Header to prepend (for platform defines, missing types)
--headerFile
./preamble.h

# Library path for DllImport
--with-librarypath
MyFunction=libmylib.so

# ClangSharp behavior config
--config
compatible-codegen
generate-native-bitfield-attribute
exclude-empty-records
exclude-funcs-with-body
log-exclusions

# Method class name (where free functions go)
--methodClassName
Apis

# Type remapping
--remap
_some_internal_tag=PublicName

# Exclude specific symbols
--exclude
_INTERNAL_ONLY_STRUCT

# Force a symbol to a specific C# type
--with-type
MY_FLAGS=uint
```

### The `--traverse` Mechanism

This is critical for custom use. Without `--traverse`, ClangSharp emits types for
**every** header transitively included. With `--traverse`, it only emits types that are
**defined in** the listed files. Transitive types are still available in the AST for
resolution — they just aren't emitted.

For win32metadata, each "partition" has a
[`main.cpp`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/partitions/Windowing/main.cpp)
with `#include` directives and a
[`settings.rsp`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/partitions/Windowing/settings.rsp)
with `--traverse` listing the headers in that partition. See the
[partitions directory](https://github.com/microsoft/win32metadata/tree/main/generation/WinSDK/partitions)
for all examples.

### The `--headerFile` (Preamble)

win32metadata injects a
[preamble header](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/partitions/header.h)
containing:

- Platform/architecture `#define`s (e.g., `_AMD64_`, `WINAPI_FAMILY`)
- Clang warning suppressions (`-Wno-pragma-pack`, etc.) — see
  [baseSettings.rsp](https://github.com/microsoft/win32metadata/blob/main/sources/GeneratorSdk/tools/assets/scraper/baseSettings.rsp)
  for the full set
- Stubs for types that are missing when parsing outside Windows

For Linux usage with non-Windows headers, the preamble is simpler — typically just
architecture defines and any missing typedefs.

### Partitioning Strategy

For a single library, one partition (one `--file` + `--namespace`) may suffice. For
larger projects with multiple logical modules, create multiple response files, each
producing a separate C# source tree under a different namespace. All sources are then
compiled together in Stage 3.

---

## Stage 2 (Optional): ConstantsScraper — `#define` Constants

### The Problem

C preprocessor `#define` macros are expanded before Clang sees them, so they do not
appear in the AST. ClangSharp cannot emit them. win32metadata solves this with a
separate regex-based tool called
[**ConstantsScraper**](https://github.com/microsoft/win32metadata/tree/main/sources/ConstantsScraper).

### When You Need It

Only if your headers define constants via `#define` that you want in the winmd. If all
constants are `const` or `enum` values, this stage is unnecessary.

### How It Works

ConstantsScraper scans header files with regex patterns to extract:

- `#define FOO 0x1234` → emitted as `public const int FOO = 0x1234;`
- Grouped constants can be synthesized into enums via an
  [`enums.json`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/enums.json) config
- Handle typedefs (e.g., `DECLARE_HANDLE(HWND)`) can be synthesized via
  [`autoTypes.json`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/autoTypes.json)

Its output is additional C# source files that get merged with ClangSharp's output before
compilation.

### For Custom Headers

Write a minimal scraper or use the existing ConstantsScraper tool (it's a .NET console
app in the win32metadata repo, build it from source). Alternatively, hand-write the
constant definitions in a `.cs` file — for small APIs this is faster.

---

## Stage 3: Emitter — C# Source → ECMA-335 WinMD

This is the core of the pipeline and the most complex part. The source lives in
[`sources/ClangSharpSourceToWinmd/`](https://github.com/microsoft/win32metadata/tree/main/sources/ClangSharpSourceToWinmd)
with helpers in
[`sources/MetadataUtils/`](https://github.com/microsoft/win32metadata/tree/main/sources/MetadataUtils).
It consists of two components:

### 3a. ClangSharpSourceCompilation

([source](https://github.com/microsoft/win32metadata/blob/main/sources/ClangSharpSourceToWinmd/ClangSharpSourceCompilation.cs))

**Purpose**: Take the raw C# output from ClangSharp and prepare it for winmd generation.

**What it does**:

1. [**NamesToCorrectNamespacesMover**](https://github.com/microsoft/win32metadata/blob/main/sources/MetadataUtils/NamesToCorrectNamespacesMover.cs)
   — Moves type declarations from the default namespace to the correct target namespace
   (driven by a
   [`requiredNamespacesForNames.rsp`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/requiredNamespacesForNames.rsp)
   map). For custom headers with a single namespace, this is typically a no-op.

2. [**MetadataSyntaxTreeCleaner**](https://github.com/microsoft/win32metadata/blob/main/sources/MetadataUtils/MetadataSyntaxTreeCleaner.cs)
   — Applies remappings and fixups to the C# syntax trees:
   - Renames types based on `--remap` entries
   - Converts enum additions (synthesize new enum members from constants)
   - Sets `[Flags]` on specified enums
   - Converts COM-style structs with vtables into proper interface shapes
   - Maps functions to correct DLL names via `staticLibs` mapping
   - Resolves `apiNamesToNamespaces` to ensure functions are in the right namespace class

3. [**CrossArchTreeMerger**](https://github.com/microsoft/win32metadata/blob/main/sources/MetadataUtils/CrossArchTreeMerger.cs)
   — Merges x86/x64/arm64 variants of the same type into a single definition with
   `#if`-style arch attributes. Only needed when scraping the same header under multiple
   target architectures.

4. **CSharpCompilation** — Creates a Roslyn `CSharpCompilation` from all the processed
   syntax trees, with `OutputKind.WindowsRuntimeMetadata` and `allowUnsafe: true`.
   Adds a `netstandard.dll` reference (for `System.Object`, `System.Attribute`, etc.).

**Key detail**: The compilation does NOT produce an output DLL. It only builds an
in-memory semantic model. The actual binary output is written by the next component.

### 3b. ClangSharpSourceWinmdGenerator

([source](https://github.com/microsoft/win32metadata/blob/main/sources/ClangSharpSourceToWinmd/ClangSharpSourceWinmdGenerator.cs))

**Purpose**: Walk the Roslyn compilation's syntax trees and write each type definition
directly into an ECMA-335
[`MetadataBuilder`](https://learn.microsoft.com/en-us/dotnet/api/system.reflection.metadata.ecma335.metadatabuilder),
producing a `.winmd` file.

**How it works**:

1. **Gather interface info** — Scan all syntax trees for structs that represent COM
   interfaces (identified by having an `lpVtbl` field and delegate members). Cache their
   method counts for vtable slot ordering.

2. **Walk all types** — A `CSharpSyntaxWalker` visits every enum, struct, class, and
   delegate in the compilation:

   | C# Construct | WinMD Output |
   |---|---|
   | `enum` | TypeDef with `value__` field + literal members |
   | `struct` | TypeDef inheriting `System.ValueType` with field layout |
   | COM `struct` (has `lpVtbl`) | TypeDef with `TypeAttributes.Interface` + abstract methods |
   | `class` with `[DllImport]` methods | TypeDef with static P/Invoke methods (`MethodImportAttributes`) |
   | `delegate` | TypeDef inheriting `System.MulticastDelegate` with `Invoke` method |

3. **Encode signatures** — Each method and field signature is encoded into ECMA-335
   `BlobEncoder` format, mapping C# types to their metadata representation (special types,
   pointers, arrays, type references).

4. **Write the PE** — Uses
   [`ManagedPEBuilder`](https://learn.microsoft.com/en-us/dotnet/api/system.reflection.portableexecutable.managedpebuilder)
   to serialize the `MetadataBuilder` into a portable executable (PE) file with the
   `.winmd` extension.

**The MetadataBuilder is the key API** — it's from
[`System.Reflection.Metadata.Ecma335`](https://learn.microsoft.com/en-us/dotnet/api/system.reflection.metadata.ecma335),
part of the standard .NET SDK. No Windows-specific APIs are used.

---

## What We Replace vs. What We Reuse

| win32metadata Component | Reuse? | Notes |
|---|---|---|
| [**ClangSharpPInvokeGenerator**](https://github.com/dotnet/ClangSharp) | ✅ Reuse as-is | .NET global tool, cross-platform, configured via `.rsp` |
| [**ConstantsScraper**](https://github.com/microsoft/win32metadata/tree/main/sources/ConstantsScraper) | ⚠️ Optional | Build from source or hand-write constants |
| [**ClangSharpSourceCompilation**](https://github.com/microsoft/win32metadata/blob/main/sources/ClangSharpSourceToWinmd/ClangSharpSourceCompilation.cs) | ✅ Reuse core logic | Need to build from source (no standalone NuGet). Simplify: skip cross-arch merge, skip Win32-specific namespace movement |
| [**ClangSharpSourceWinmdGenerator**](https://github.com/microsoft/win32metadata/blob/main/sources/ClangSharpSourceToWinmd/ClangSharpSourceWinmdGenerator.cs) | ✅ Reuse core logic | The ECMA-335 writer. Build from source. No Windows dependencies. |
| [**NamesToCorrectNamespacesMover**](https://github.com/microsoft/win32metadata/blob/main/sources/MetadataUtils/NamesToCorrectNamespacesMover.cs) | ⚠️ Simplify | For single-namespace projects, not needed |
| [**MetadataSyntaxTreeCleaner**](https://github.com/microsoft/win32metadata/blob/main/sources/MetadataUtils/MetadataSyntaxTreeCleaner.cs) | ⚠️ Simplify | Keep remap + enum synthesis; drop Win32-specific COM fixups if not needed |
| [**CrossArchTreeMerger**](https://github.com/microsoft/win32metadata/blob/main/sources/MetadataUtils/CrossArchTreeMerger.cs) | ❌ Skip | Only needed for multi-arch Windows SDK scraping |
| [**WinmdGenerator MSBuild SDK**](https://github.com/microsoft/win32metadata/tree/main/sources/GeneratorSdk) | ❌ Replace | This is the Windows-only orchestration layer |
| [**DoAll.ps1**](https://github.com/microsoft/win32metadata/blob/main/scripts/DoAll.ps1) / PowerShell scripts | ❌ Replace | Replaced by bindscrape CLI |
| **Windows SDK NuGet packages** | ❌ Not needed | Only contain Windows headers |
| **MIDL compiler** | ❌ Not needed | Only used for WinRT IDL, not for C headers |
| **`.lib` file scanning** | ❌ Not needed | Only maps Win32 functions to DLLs via [`libMappings.rsp`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/libMappings.rsp); for custom libs, use `--with-librarypath` |

---

## bindscrape Pipeline Design

### Input

```
project/
├── headers/
│   ├── mylib.h
│   └── mylib_types.h
├── bindscrape.toml        # Project configuration
└── preamble.h             # (optional) Platform defines, missing types
```

### Configuration (`bindscrape.toml`)

```toml
[output]
name = "MyLibrary"           # Assembly name in winmd
version = "1.0.0.0"
path = "output/MyLibrary.winmd"

[scraper]
language = "c"               # "c" or "c++"
include_dirs = ["headers/", "/usr/include"]
preamble = "preamble.h"

[[partition]]
name = "Core"
namespace = "MyLibrary.Core"
file = "headers/mylib.h"
traverse = ["headers/mylib.h", "headers/mylib_types.h"]
library = "libmylib.so"

[[partition]]
name = "Extensions"
namespace = "MyLibrary.Extensions"
file = "headers/mylib_ext.h"
traverse = ["headers/mylib_ext.h"]
library = "libmylib_ext.so"

[remaps]
"_internal_tag" = "PublicName"

[constants]
# Optional: manually define #define constants
MY_VERSION = { type = "int", value = "3" }

[enums]
# Optional: synthesize enums from constants
MY_FLAGS = { type = "uint", members = ["FLAG_A", "FLAG_B", "FLAG_C"], flags = true }
```

### Execution Steps

```
bindscrape generate
```

1. **Generate response files** — For each `[[partition]]`, produce a `.rsp` file with
   the correct `--file`, `--namespace`, `--traverse`, `--with-librarypath`, etc.

2. **Run ClangSharp** — Invoke `ClangSharpPInvokeGenerator @partition.rsp` for each
   partition. Output: C# source files in `obj/generated/<partition>/`.

3. **Generate constants** (if configured) — Emit additional `.cs` files for `#define`
   constants, either via ConstantsScraper or from the `[constants]`/`[enums]` config.

4. **Compile & emit winmd** — Load all generated `.cs` files into a Roslyn
   `CSharpCompilation`. Apply remaps (via MetadataSyntaxTreeCleaner or a simplified
   version). Pass the compilation to `ClangSharpSourceWinmdGenerator` to produce
   the `.winmd` file.

### Simplified Emitter

For custom (non-Win32) headers, the Emitter can be significantly simplified:

- **No cross-arch merge** — Scrape for one architecture (the host). If multi-arch is
  needed later, it can be added.
- **No Win32-specific namespace movement** — Use the namespace from ClangSharp directly.
- **No `.lib` scanning** — Library names come from `--with-librarypath` in the `.rsp`.
- **No Win32-specific COM interface fixups** — Unless the custom headers define
  COM-style interfaces, the vtable-to-interface conversion is not needed.

The minimum viable Emitter is:

```csharp
// Pseudocode for the simplified pipeline
var trees = Directory.GetFiles("obj/generated", "*.cs", SearchOption.AllDirectories)
    .Select(f => CSharpSyntaxTree.ParseText(File.ReadAllText(f), null, f));

var compilation = CSharpCompilation.Create(
    null,
    trees,
    new[] { MetadataReference.CreateFromFile(netstandardDllPath) },
    new CSharpCompilationOptions(OutputKind.WindowsRuntimeMetadata, allowUnsafe: true));

// Apply remaps if needed
// trees = trees.Select(t => MetadataSyntaxTreeCleaner.CleanSyntaxTree(t, remaps, ...));

var sourceCompilation = new ClangSharpSourceCompilation(compilation, typeImports);
var generator = ClangSharpSourceWinmdGenerator.GenerateWindmdForCompilation(
    sourceCompilation,
    typeImports: new Dictionary<string, string>(),
    reducePointerLevels: new HashSet<string>(),
    forceGuidConsts: new HashSet<string>(),
    version: new Version(1, 0, 0, 0),
    outputFileName: "output/MyLibrary.winmd");
```

---

## Dependencies

| Dependency | Version | Platform | Purpose |
|---|---|---|---|
| .NET SDK | 8.0+ | Linux/macOS/Windows | Runtime for all tools |
| [ClangSharpPInvokeGenerator](https://www.nuget.org/packages/ClangSharpPInvokeGenerator) | 17.x | All (bundles libClang) | C header parsing |
| [Microsoft.CodeAnalysis.CSharp](https://www.nuget.org/packages/Microsoft.CodeAnalysis.CSharp) | 4.x | All | Roslyn C# compilation |
| [System.Reflection.Metadata](https://www.nuget.org/packages/System.Reflection.Metadata) | 8.0+ | All | ECMA-335 MetadataBuilder |
| netstandard.dll (2.1) | 2.1 | All | Reference assembly for compilation |
| [ClangSharpSourceToWinmd](https://github.com/microsoft/win32metadata/tree/main/sources/ClangSharpSourceToWinmd) | from source | All | Winmd writer (build from win32metadata repo) |
| [MetadataUtils](https://github.com/microsoft/win32metadata/tree/main/sources/MetadataUtils) | from source | All | Helper types (InterfaceInfo, StructInfo, etc.) |

All dependencies are .NET libraries with no Windows-specific native code (except
libClang, which is bundled per-platform by the ClangSharp NuGet).

---

## Build Strategy for Reused Components

The [`ClangSharpSourceToWinmd`](https://github.com/microsoft/win32metadata/tree/main/sources/ClangSharpSourceToWinmd)
and [`MetadataUtils`](https://github.com/microsoft/win32metadata/tree/main/sources/MetadataUtils)
projects from win32metadata are not published as standalone NuGet packages. Options:

### Option A: Git Submodule + Source Reference

Add `microsoft/win32metadata` as a git submodule. Reference the `ClangSharpSourceToWinmd`
and `MetadataUtils` projects directly:

```xml
<ProjectReference Include="../win32metadata/sources/ClangSharpSourceToWinmd/ClangSharpSourceToWinmd.csproj" />
<ProjectReference Include="../win32metadata/sources/MetadataUtils/MetadataUtils.csproj" />
```

**Pros**: Always up to date, minimal code duplication.
**Cons**: Pulls in the entire win32metadata repo; may bring unwanted transitive dependencies.

### Option B: Fork + Extract

Fork the relevant source files into bindscrape:

- [`ClangSharpSourceWinmdGenerator.cs`](https://github.com/microsoft/win32metadata/blob/main/sources/ClangSharpSourceToWinmd/ClangSharpSourceWinmdGenerator.cs) (~1,500 lines)
- [`ClangSharpSourceCompilation.cs`](https://github.com/microsoft/win32metadata/blob/main/sources/ClangSharpSourceToWinmd/ClangSharpSourceCompilation.cs) (~300 lines)
- [`MetadataSyntaxTreeCleaner.cs`](https://github.com/microsoft/win32metadata/blob/main/sources/MetadataUtils/MetadataSyntaxTreeCleaner.cs)
- [`NamesToCorrectNamespacesMover.cs`](https://github.com/microsoft/win32metadata/blob/main/sources/MetadataUtils/NamesToCorrectNamespacesMover.cs)
- Supporting types from [`MetadataUtils`](https://github.com/microsoft/win32metadata/tree/main/sources/MetadataUtils)

Strip Win32-specific logic (COM interface detection heuristics, Win32 string type
remapping, `PWSTR`/`PSTR` special cases, etc.). This produces a leaner, general-purpose
winmd emitter.

**Pros**: Full control, smaller footprint, no Win32 assumptions.
**Cons**: Maintenance burden if upstream changes.

### Recommended: Option B

The win32metadata Emitter has extensive Win32-specific hardcoding (e.g., special-casing
`PWSTR`, `PSTR`, `HRESULT`, `SECURITY_STATUS`, Win32 handle types). A fork-and-simplify
approach avoids carrying this baggage and produces a cleaner tool.

---

## Win32-Specific Code to Strip When Forking

When forking `ClangSharpSourceWinmdGenerator.cs`, the following Win32-specific
hardcoding must be removed or generalized. These are scattered throughout the type
encoding and signature-writing logic:

### Special-Cased Types in the Generator

The generator has hardcoded `if` / `switch` branches for these Win32 types:

| Hardcoded Type | What It Does | Action for bindscrape |
|---|---|---|
| `PWSTR` / `PSTR` | Encoded as `System.String` with specific marshaling in winmd, not as a raw `char*` pointer | Remove — encode as pointer unless user configures a string mapping |
| `HRESULT` | Mapped to a special `Windows.Foundation.HResult`-style TypeRef | Remove — treat as `int` or user-defined struct |
| `NTSTATUS` | Similar special handling to HRESULT | Remove |
| `SECURITY_STATUS` | Similar special handling | Remove |
| `BOOL` / `BOOLEAN` | Mapped to specific interop boolean types | Remove — treat as `int` / `byte` |
| `PCWSTR` / `PCSTR` | Const string pointer variants | Remove |
| `IUnknown` / `IDispatch` | Referenced as well-known COM base types from `Windows.Win32.System.Com` namespace | Remove or generalize — only relevant if COM interfaces are present |
| `GUID` | Encoded as `System.Guid` | Keep — this is a standard .NET type |
| `LARGE_INTEGER` / `ULARGE_INTEGER` | Mapped to `long` / `ulong` | Remove |
| Win32 handle types (`HWND`, `HINSTANCE`, etc.) | Recognized by `IsHandle()` heuristic; get special struct wrapping | Remove — bindscrape treats them as opaque struct wrappers naturally |

### Namespace Hardcoding

The generator assumes `Windows.Win32.*` namespace prefixes in several places:
- `Windows.Win32.Foundation` for base types
- `Windows.Win32.System.Com` for COM interfaces
- Attribute types like `SupportedArchitectureAttribute`, `NativeTypedefAttribute`, etc.
  are expected in `Windows.Win32.Foundation.Metadata`

For bindscrape, these must be parameterized or moved to a user-defined root namespace.
The attributes (`NativeTypedefAttribute`, `SupportedOSPlatformAttribute`, etc.) need to
be defined in the generated output or a companion assembly.

### COM Interface Detection Heuristic

The generator identifies COM interfaces by checking if a `struct` has:
1. A field named `lpVtbl` (or `lpVtbl*`)
2. Nested `delegate` members representing vtable slots

This heuristic is Win32-specific but actually useful for any C library using COM-style
vtables. **Keep this logic** but make it opt-in via config.

---

## Key API Signatures (for Implementation Reference)

These are the exact entry points we need to call or fork. Captured here to avoid
re-reading the source later.

### ClangSharpSourceCompilation.Create()

```csharp
public static ClangSharpSourceCompilation Create(
    string sourceDirectory,           // path to generated .cs files
    string arch,                      // "x64", "x86", "arm64"
    Dictionary<string, string> remaps,
    Dictionary<string, string> enumAdditions,       // const → enum member mappings
    Dictionary<string, string> typeImports,          // type → external winmd references
    Dictionary<string, string> requiredNamespaces,   // type → correct namespace
    HashSet<string> reducePointerLevels,             // types needing pointer reduction
    IReadOnlyCollection<string> addedRefs,           // additional assembly references
    Dictionary<string, string> staticLibs,           // function → DLL name mapping
    Dictionary<string, string> apiNamesToNamespaces  // function → namespace override
)
```

**For bindscrape**: `typeImports`, `requiredNamespaces`, `apiNamesToNamespaces` can
all be empty dictionaries initially. `staticLibs` is replaced by `--with-librarypath`
in the `.rsp`. `arch` should default to host architecture.

### ClangSharpSourceWinmdGenerator.GenerateWindmdForCompilation()

```csharp
public static void GenerateWindmdForCompilation(
    ClangSharpSourceCompilation compilation,
    Dictionary<string, string> typeImports,
    HashSet<string> reducePointerLevels,
    HashSet<string> forceGuidConsts,     // GUIDs to emit as named constants
    Version version,                     // assembly version
    string outputFileName                // output .winmd path
)
```

### netstandard.dll Discovery

The compilation needs `netstandard.dll` as a reference. The upstream code looks for it at:

```
// Windows (hardcoded in ClangSharpSourceCompilation.cs)
$ProgramFiles/dotnet/packs/NETStandard.Library.Ref/2.1.0/ref/netstandard2.1/netstandard.dll

// Linux equivalent
$DOTNET_ROOT/packs/NETStandard.Library.Ref/2.1.0/ref/netstandard2.1/netstandard.dll
// or typically:
/usr/share/dotnet/packs/NETStandard.Library.Ref/2.1.0/ref/netstandard2.1/netstandard.dll
// or via snap:
/snap/dotnet-sdk/current/packs/NETStandard.Library.Ref/2.1.0/ref/netstandard2.1/netstandard.dll
```

bindscrape should discover this via:
1. `DOTNET_ROOT` environment variable
2. `dotnet --info` output (look for "Base Path")
3. Well-known paths (`/usr/share/dotnet`, `/usr/lib/dotnet`)
4. User-configurable override in `bindscrape.toml`

### OutputKind.WindowsRuntimeMetadata

The Roslyn compilation must use `OutputKind.WindowsRuntimeMetadata` — this tells Roslyn
to produce a semantic model compatible with WinRT/winmd conventions. This is the
critical flag that makes the output a `.winmd` rather than a `.dll`. Despite the
"Windows" in the name, this is a pure .NET enum value with no platform dependency.

---

## Open Questions / Decisions for Implementation

These are unresolved design decisions to address when starting implementation:

### Q1: bindscrape CLI Language — Rust or .NET?

The workspace is at `rs/bindscrape`, suggesting Rust. Options:

| Approach | Pros | Cons |
|---|---|---|
| **Rust CLI + subprocess .NET** | Native binary, fast startup, TOML parsing trivial in Rust | Must shell out to `dotnet run` or a published .NET exe for the Emitter; two toolchains |
| **Pure .NET CLI** | Single toolchain, direct API access to Roslyn + MetadataBuilder | Slower startup, larger deployment, breaks the "rs" convention |
| **Rust CLI + .NET library via interop** | Best of both | Complex FFI bridge, high engineering cost |

**Likely best**: Rust CLI that generates `.rsp` files / invokes ClangSharp, then shells
out to a small .NET console app (the forked Emitter) for the C# → winmd step. The Rust
CLI handles TOML config, file discovery, and orchestration. The .NET app is a thin
wrapper around the forked Emitter.

### Q2: typeImports — Cross-winmd Type References

win32metadata uses `typeImports` to reference types defined in *other* winmd files (e.g.,
WinRT foundation types). For bindscrape v1, this can be empty. But if someone generates
multiple winmd files (one per library) and wants cross-references, typeImports provides
the mechanism. Worth supporting eventually.

### Q3: Attribute Types

The winmd format uses custom attributes like:
- `NativeTypedefAttribute` — marks a struct as a typedef wrapper
- `NativeBitfieldAttribute` — describes bitfield layout
- `SupportedArchitectureAttribute` — marks arch-specific types
- `SupportedOSPlatformAttribute` — marks minimum OS version
- `ConstAttribute` — marks const pointer parameters
- `FlexibleArrayAttribute` — marks trailing flexible array members

These are defined in the `Windows.Win32.Foundation.Metadata` namespace in Win32's winmd.
For bindscrape, they need to be either:
- Defined inline in the generated winmd (as TypeDefs)
- Left as TypeRefs pointing to a shared metadata assembly
- Omitted if the consumer doesn't need them

**Decision needed**: Which attributes are essential for consumers like `windows-rs`?

### Q4: How windows-rs Consumes winmd

[windows-rs](https://github.com/microsoft/windows-rs) uses the
[`windows-bindgen`](https://github.com/microsoft/windows-rs/tree/master/crates/libs/bindgen)
crate to read winmd files and generate Rust bindings. It expects:
- Standard ECMA-335 TypeDef/TypeRef/MethodDef tables
- `[DllImport]` (technically `ImplMap` table) for P/Invoke functions
- Struct layout via `ClassLayout` and `FieldLayout` tables
- Enum members as `Literal` fields
- COM interfaces with `InterfaceImpl` and method ordering matching vtable order
- The custom attributes listed above (especially `NativeTypedefAttribute`)

If bindscrape's output matches these conventions, `windows-bindgen` can consume it
directly. This is the primary integration target.

### Q5: Testing Strategy

- **Round-trip test**: Parse a known `.h` file → generate winmd → read it back with
  `System.Reflection.Metadata` and verify types/methods/fields match expectations
- **Comparison test**: For Win32 headers, compare bindscrape output against the official
  `Windows.Win32.winmd` to verify compatibility
- **Integration test**: Feed the winmd to `windows-bindgen` and verify it generates
  compilable Rust code

---

## Limitations and Considerations

### `#define` Constants
ClangSharp cannot extract `#define` macros. For custom headers, either:
- Use ConstantsScraper (build from win32metadata source)
- Hand-write constants in a `.cs` file
- Use a simple regex script to extract them

### Function-Like Macros
Not representable in winmd. Must be manually wrapped as inline functions in a shim header,
or ignored.

### C++ Templates / Overloaded Functions
ClangSharp supports C++ but templates and overloads require careful handling. Stick to
C-compatible headers (extern "C") for reliable results.

### Multi-Architecture
The initial version targets a single architecture (host). Multi-arch support
(cross-arch merging à la win32metadata) can be added later by running ClangSharp
per-arch and using `CrossArchTreeMerger`.

### COM Interfaces
If headers define COM-style interfaces (structs with vtable pointers), the existing
vtable-to-interface conversion logic in the Emitter handles this. For non-COM headers,
this path is unused.

### netstandard.dll Location
The Emitter needs a reference to `netstandard.dll` (2.1). On Linux, this is typically at:
```
$DOTNET_ROOT/packs/NETStandard.Library.Ref/2.1.0/ref/netstandard2.1/netstandard.dll
```
bindscrape should locate this automatically via `dotnet --list-runtimes` or the
`DOTNET_ROOT` environment variable.

---

## Summary

bindscrape reuses two proven, cross-platform components from win32metadata:

1. **ClangSharp** — Real C/C++ parsing via libClang, emitting C# P/Invoke source
2. **The Emitter** — Roslyn compilation + `MetadataBuilder` ECMA-335 writer

It replaces the Windows-only orchestration (MSBuild SDK, PowerShell, Windows SDK NuGet,
MIDL) with a simple TOML-driven CLI. The result is a tool that runs on Linux and
produces `.winmd` files from arbitrary C headers — the same binary format consumed by
language projections like [`windows-rs`](https://github.com/microsoft/windows-rs),
[CsWin32](https://github.com/microsoft/CsWin32), and others.

---

## Reference Links

| Resource | URL |
|---|---|
| win32metadata repo | https://github.com/microsoft/win32metadata |
| win32metadata architecture doc | https://github.com/microsoft/win32metadata/blob/main/docs/architecture.md |
| ClangSharp repo | https://github.com/dotnet/ClangSharp |
| ClangSharp NuGet | https://www.nuget.org/packages/ClangSharpPInvokeGenerator |
| Emitter source | https://github.com/microsoft/win32metadata/tree/main/sources/ClangSharpSourceToWinmd |
| MetadataUtils source | https://github.com/microsoft/win32metadata/tree/main/sources/MetadataUtils |
| ConstantsScraper source | https://github.com/microsoft/win32metadata/tree/main/sources/ConstantsScraper |
| GeneratorSdk (MSBuild SDK) | https://github.com/microsoft/win32metadata/tree/main/sources/GeneratorSdk |
| Partitions directory | https://github.com/microsoft/win32metadata/tree/main/generation/WinSDK/partitions |
| Base scraper settings | https://github.com/microsoft/win32metadata/blob/main/sources/GeneratorSdk/tools/assets/scraper/baseSettings.rsp |
| Scraper settings (WinSDK) | https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/scraper.settings.rsp |
| Emitter settings (WinSDK) | https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/emitter.settings.rsp |
| enums.json | https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/enums.json |
| autoTypes.json | https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/autoTypes.json |
| libMappings.rsp | https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/libMappings.rsp |
| requiredNamespacesForNames.rsp | https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/requiredNamespacesForNames.rsp |
| Windows.Win32.proj | https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/Windows.Win32.proj |
| DoAll.ps1 | https://github.com/microsoft/win32metadata/blob/main/scripts/DoAll.ps1 |
| Issue #998 (custom winmd) | https://github.com/microsoft/win32metadata/issues/998 |
| Issue #1589 (Linux case) | https://github.com/microsoft/win32metadata/issues/1589 |
| ECMA-335 MetadataBuilder API | https://learn.microsoft.com/en-us/dotnet/api/system.reflection.metadata.ecma335.metadatabuilder |
| ManagedPEBuilder API | https://learn.microsoft.com/en-us/dotnet/api/system.reflection.portableexecutable.managedpebuilder |
