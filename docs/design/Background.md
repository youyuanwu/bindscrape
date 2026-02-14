# Background: How win32metadata Generates .winmd from C Headers

## Overview

The [microsoft/win32metadata](https://github.com/microsoft/win32metadata) project produces
`Windows.Win32.winmd` — a machine-readable metadata file describing the entire Win32 API
surface. Language projections (C#, Rust, Dart, Zig, etc.) consume this winmd to
auto-generate idiomatic bindings, replacing the historically manual and error-prone
process of hand-writing P/Invoke signatures or FFI declarations.

The winmd file uses the **ECMA-335** binary format (the same format as .NET assemblies and
WinRT winmd files), chosen because:

- WinRT already uses this format for metadata
- Many Win32 concepts (structs, function pointers, COM interfaces) already have .NET
  interop precedents in ECMA-335
- Reflection-based APIs make it simple to parse and convert to other formats (JSON, etc.)

## Architecture

The pipeline has three layers:

```
C/C++ Headers (Windows SDK)
        │
        ▼
   ┌──────────┐
   │  Scraper  │   ClangSharp + ConstantsScraper
   └─────┬─────┘
         │  C# source files
         ▼
   ┌──────────┐
   │  Emitter  │   ClangSharpSourceCompilation → ClangSharpSourceWinmdGenerator
   └─────┬─────┘
         │  ECMA-335 binary
         ▼
   Windows.Win32.winmd
```

The entire pipeline is packaged as an MSBuild project SDK called
[Microsoft.Windows.WinmdGenerator](https://www.nuget.org/packages/Microsoft.Windows.WinmdGenerator/),
driven by a project file
([Windows.Win32.proj](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/Windows.Win32.proj)).
A full build is triggered by running `./DoAll.ps1 -Clean`.

---

## Layer 1: Scraper — C Headers → C# Source

### ClangSharp (PInvokeGenerator)

[ClangSharp](https://github.com/dotnet/ClangSharp) is the core engine. It is a .NET tool
that wraps **libClang** (LLVM's C API to the Clang compiler) to parse C/C++ headers and
generate C# P/Invoke bindings. Because it uses a real compiler front-end, it builds a
full AST — it understands structs, functions, enums, COM interfaces, typedefs, function
pointers, bitfields, nested types, and more.

ClangSharp is invoked via response files (`.rsp`) which configure its behavior:

- **Base settings:**
  [`baseSettings.rsp`](https://github.com/microsoft/win32metadata/blob/main/sources/GeneratorSdk/tools/assets/scraper/baseSettings.rsp)
  plus architecture-specific response files.
- **Project settings:**
  [`scraper.settings.rsp`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/scraper.settings.rsp)
  along with domain-specific files like
  [`libMappings.rsp`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/libMappings.rsp),
  [`supportedOS.rsp`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/supportedOS.rsp),
  and
  [`WithSetLastError.rsp`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/WithSetLastError.rsp).

The output is C# source files written to `generation/WinSDK/obj/generated/`.

### Partitions

The Windows SDK headers are organized into **Partitions** under
[`generation/WinSDK/Partitions/`](https://github.com/microsoft/win32metadata/tree/main/generation/WinSDK/Partitions).
Each partition is a folder containing two files:

**`main.cpp`** — `#include` statements for the target headers and their dependencies, in
the correct order. For example, the Registry partition:

```cpp
#include "intrinfix.h"
#include "windows.fixed.h"

#include <wtypes.h>
#include <winbase.h>
#include <winnt.h>
#include <winreg.h>
#include <statehelpers.h>
#include <regstr.h>
```

**`settings.rsp`** — A response file specifying which headers to traverse and the target
namespace:

```
--traverse
<IncludeRoot>/um/winreg.h
<IncludeRoot>/um/statehelpers.h
<IncludeRoot>/um/regstr.h
--namespace
Windows.Win32.System.Registry
```

A header file can only belong to one partition. The `--traverse` flag tells ClangSharp
which headers to actually emit bindings for (as opposed to headers that are merely
included for type resolution).

### ConstantsScraper

C `#define` macros are not AST nodes — they are preprocessor directives that Clang
expands before parsing. ClangSharp therefore cannot extract them. A separate tool called
[ConstantsScraper](https://github.com/microsoft/win32metadata/blob/main/sources/MetadataUtils/ConstantsScraper.cs)
fills this gap by walking the same header files and extracting constants via **regular
expression pattern matching**.

- Base settings:
  [`baseSettings.ConstantsScraper.rsp`](https://github.com/microsoft/win32metadata/blob/main/sources/GeneratorSdk/tools/assets/scraper/baseSettings.ConstantsScraper.rsp)
- Project settings:
  [`ConstantsScraper.settings.rsp`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/ConstantsScraper.settings.rsp)
  (controls type overrides, exclusions, and attribute annotations for constants)

Constants that cannot be extracted by regex can be manually defined as `.cs` files in
[`generation/WinSDK/manual/`](https://github.com/microsoft/win32metadata/tree/main/generation/WinSDK/manual).

---

## Layer 2: Emitter — C# Source → .winmd

### ClangSharpSourceCompilation

This class orchestrates manipulation and compilation of the generated C# files. It
applies several AST transformations:

- **`NamesToCorrectNamespacesMover`** — Moves APIs to their correct namespaces based on
  [`requiredNamespacesForNames.rsp`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/requiredNamespacesForNames.rsp).
  This handles the case where a header's APIs must be split across multiple namespaces.

- **`MetadataSyntaxTreeCleaner`** — Visits each node in the C# AST and applies
  modifications: type remaps, custom attributes, enum substitutions (from
  [`enums.json`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/enums.json)),
  and handle typedef definitions (from
  [`autoTypes.json`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/autoTypes.json)).
  It also converts ClangSharp's COM structs into proper COM interfaces (ClangSharp emits
  COM objects as structs because the CLR disallows managed types like `interface` on
  unsafe structs, but winmd has no such restriction).

- **`CrossArchTreeMerger`** — Merges C# files generated from multiple architectures
  (x86, x64, arm64) to identify architecture-specific APIs and produce a unified output.

Project-specific emitter settings live in
[`emitter.settings.rsp`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/emitter.settings.rsp),
which controls attribute application and member remappings.

### ClangSharpSourceWinmdGenerator

This class walks the final C# AST and writes each node into the ECMA-335 binary format,
producing `Windows.Win32.winmd`. The emitter augments standard ECMA-335 with additional
patterns and custom attributes that encode Win32-specific semantics (e.g., `SetLastError`,
`SupportedOSPlatform`, `NativeTypeName`) so that language projections can provide an
improved developer experience.

---

## Key Customization Files

| File | Purpose |
|------|---------|
| `scraper.settings.rsp` | ClangSharp options: type remaps, exclusions, overrides |
| `emitter.settings.rsp` | Attributes and member remaps applied during emission |
| `enums.json` | Defines enums synthesized from `#define` constants (e.g., `WNDCLASS_STYLES` from `CS_*` in `WinUser.h`) |
| `autoTypes.json` | Defines handle typedefs (e.g., `HWND`, `HKEY`) with their close functions and invalid values |
| `libMappings.rsp` | Maps functions to their import DLLs (prepopulated by scanning SDK `.lib` files) |
| `requiredNamespacesForNames.rsp` | Per-API namespace overrides for splitting headers across namespaces |
| `manual/*.cs` | Hand-written C# for things that cannot be auto-scraped |
| `scraper.header.txt` | `using` statements added to generated C# files for cross-namespace resolution |
| `ConstantsScraper.settings.rsp` | Type annotations, exclusions, and attributes for scraped constants |

---

## Enum Generation

Enums are defined in
[`enums.json`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/enums.json).
Each entry specifies:

- `name` — The enum name (e.g., `WNDCLASS_STYLES`)
- `autoPopulate.header` / `autoPopulate.filter` — Header file and prefix/regex to scan
  for matching constants (e.g., `CS_` in `WinUser.h`)
- `members` — Manually defined members (can be combined with `autoPopulate`)
- `flags` — Whether this is a `[Flags]` enum
- `uses` — Where the enum should be applied: struct fields, method parameters, return
  values, or COM interface methods

When an enum is applied, the original loosely-typed `uint` parameter or field is replaced
with the strongly-typed enum, improving discoverability and type safety. Constants that
become enum members are removed from the standalone constants list.

## Typedef / Handle Generation

Handle typedefs are defined in
[`autoTypes.json`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/autoTypes.json).
Each entry specifies:

- `Name` — The typedef name (e.g., `BCRYPT_KEY_HANDLE`)
- `ValueType` — The underlying type (`void*`, or special patterns like `DECLARE_HANDLE`)
- `CloseApi` — The function that disposes the handle (e.g., `BCryptDestroyKey`)
- `InvalidHandleValues` — Values that represent an invalid handle (e.g., `0`, `-1`)
- `NativeTypedef` — Whether the typedef exists in the native headers (vs. being a
  metadata-only addition for improved type safety)

Language projections can use this information to generate safe wrappers (e.g., C#
`SafeHandle`, Rust `Drop` implementations).

---

## Linux / Cross-Platform Viability

The win32metadata pipeline **does not run on Linux** and is not designed to. The
maintainers have stated they have "no intention of trying to get the whole pipeline
running on Linux" ([issue #1589](https://github.com/microsoft/win32metadata/issues/1589)).

### Windows-Only Dependencies

The pipeline has several hard dependencies on Windows:

1. **Visual Studio 2022** — The build entry point (`DoAll.ps1`) immediately calls
   `Get-VSPath.ps1`, which requires `vswhere.exe` and specific VS workloads
   (VC++ x86/x64 tools, VC++ ARM64 tools, Windows 11 SDK 26100). The development
   environment setup (`configuration.dsc.yaml`) uses WinGet to install VS Community.

2. **Windows SDK headers** — The actual content being scraped. The `sdk.props` file
   references the NuGet package `Microsoft.Windows.SDK.CPP` (version 10.0.19041.5) which
   provides the Windows SDK include files. The project also ships a
   `RecompiledIdlHeaders/` directory containing pre-compiled IDL headers. These are just
   header files (not binaries), so they are technically platform-agnostic content, but the
   NuGet package is designed for the Windows SDK workflow.

3. **`TargetPlatformIdentifier=Windows`** — Set explicitly in `sdk.props`, which may
   cause MSBuild to apply Windows-specific SDK resolution logic.

4. **PowerShell scripts** — The `ScrapeHeaders` MSBuild task calls PowerShell scripts
   (e.g., `Install-DotNetTool.ps1`) via `TaskUtils.CallPowershellScript`. Historically
   these invoked `powershell.exe` (Windows PowerShell 5.1) rather than `pwsh` (PowerShell
   Core / cross-platform).

5. **MIDL compiler** — The `CompileIdls` task calls the MIDL compiler from the Windows
   SDK bin directory to compile `.idl` files into headers. MIDL is Windows-only.

6. **`ScanLibs` task** — Scans Windows `.lib` import libraries to map functions to DLLs.
   These `.lib` files are Windows-only artifacts.

### What Is Cross-Platform

The individual tools in the pipeline are technically cross-platform:

- **ClangSharp / `ClangSharpPInvokeGenerator`** — A .NET tool wrapping libClang. It works
  on Linux (libClang has Linux builds). This is the core C header → C# transformation
  engine.

- **MSBuild / .NET 8 SDK** — Cross-platform. The MSBuild project files could in
  principle be evaluated on Linux.

- **Emitter (C# → winmd)** — Pure .NET code that compiles C# syntax trees into ECMA-335
  binary. No OS dependency.

- **ConstantsScraper** — Pure .NET regex-based tool. No OS dependency.

### Case Sensitivity Issue

Issue [#1589](https://github.com/microsoft/win32metadata/issues/1589) reported that on
Linux (case-sensitive filesystem), the MSBuild SDK import `Sdk.props` failed because the
file was actually named `sdk.props`. This was fixed by renaming to the correct casing, but
the reporter confirmed that even after this fix, the pipeline fails at the next step
(PowerShell invocation), and likely at many steps after that.

### Implications for bindscrape

If we want to build a similar pipeline that runs on Linux, we need to:

- Use ClangSharp (or libClang directly) for header parsing — this works on Linux
- Replace PowerShell scripts with platform-agnostic orchestration
- Replace MIDL compilation with an alternative (or skip it if targeting non-COM headers)
- Replace `.lib` scanning with an alternative mechanism for mapping functions to shared
  libraries (e.g., parsing `.so` files, or using a manually curated mapping)
- Provide header files directly rather than depending on the `Microsoft.Windows.SDK.CPP`
  NuGet package

---

## Using WinmdGenerator for Custom C Headers

The win32metadata FAQ explicitly says: **"Yes. The same tooling we use to produce metadata
for Win32 APIs can be used to produce metadata for your own APIs."** The
[`Microsoft.Windows.WinmdGenerator`](https://www.nuget.org/packages/Microsoft.Windows.WinmdGenerator/)
NuGet package is published as an MSBuild project SDK for exactly this purpose.

### The Documented Workflow (Windows-Only)

Rafael Rivera (riverar) wrote a detailed walkthrough at
[withinrafael.com](https://withinrafael.com/2023/01/18/generating-metadata-for-the-windows-crate)
demonstrating how to generate a winmd from the DIA SDK headers and then produce a Rust
crate from it. The steps are:

1. Create an MSBuild project file referencing the WinmdGenerator SDK:
   ```xml
   <Project Sdk="Microsoft.Windows.WinmdGenerator/0.59.13-preview">
     <PropertyGroup>
       <OutputWinmd>output/MyLib.winmd</OutputWinmd>
       <WinmdVersion>1.0.0.0</WinmdVersion>
     </PropertyGroup>
     <ItemGroup>
       <Headers Include="path/to/myheader.h" />
       <Partition Include="main.cpp">
         <TraverseFiles>@(Headers)</TraverseFiles>
         <Namespace>MyLib</Namespace>
       </Partition>
     </ItemGroup>
   </Project>
   ```

2. Create `main.cpp` with `#include` directives for your headers.

3. Run `dotnet build` — the SDK orchestrates ClangSharp → C# → winmd automatically.

4. Use the resulting `.winmd` with language projections (e.g., `windows-bindgen` for
   Rust, CsWin32 for C#).

There are several sample projects in the repo at
[`sources/GeneratorSdk/samples/`](https://github.com/microsoft/win32metadata/tree/main/sources/GeneratorSdk/samples)
including `DiaSdk`, `DWriteCoreSample`, `WebView2Sample`, and a generic
`CppProjectForScraping`.

### Why It Does Not Work on Linux

Even though the WinmdGenerator SDK is "just" an MSBuild SDK and MSBuild is cross-platform,
the pipeline fails on Linux for the reasons documented above:

1. **`sdk.props` casing** — Fixed (renamed to `Sdk.props`), but was just the first
   roadblock.

2. **PowerShell invocation** — The `ScrapeHeaders` task calls `Install-DotNetTool.ps1`
   via `TaskUtils.CallPowershellScript`, which historically hardcoded `powershell.exe`.

3. **Windows SDK NuGet dependency** — `sdk.props` unconditionally references
   `Microsoft.Windows.SDK.CPP` (version 10.0.19041.5) and
   `Microsoft.Windows.SDK.Win32Metadata` as `<PackageReference>` items. Even if you set
   `<UseWinSDKAssets>false</UseWinSDKAssets>` to skip the WinSDK assets, the package
   references are still there in `sdk.props`.

4. **`TargetPlatformIdentifier=Windows`** — Set unconditionally in `sdk.props`.

5. **MIDL compiler** — If your headers involve IDL files, the `CompileIdls` task requires
   the Windows-only MIDL compiler.

6. **Import lib scanning** — `ScanLibs` expects Windows `.lib` files. Without them,
   functions won't get `[DllImport]` attributes in the winmd.

### What Would Be Needed for Linux

The core ClangSharp tool itself works on Linux — a user in
[issue #998](https://github.com/microsoft/win32metadata/issues/998) confirmed they could
install `ClangSharpPInvokeGenerator` on Linux and start parsing headers (though Windows
SDK headers like `windows.h` obviously wouldn't be found). The problem is all the
orchestration around it.

To scrape arbitrary (non-Windows) C headers on Linux and produce a winmd, you would need
to either:

**Option A: Patch the WinmdGenerator SDK**
- Fix PowerShell calls to use `pwsh`
- Remove or conditionalize the `Microsoft.Windows.SDK.CPP` package reference
- Remove `TargetPlatformIdentifier=Windows`
- Skip `ScanLibs` / provide lib mappings manually
- This is fragile and would need to be re-done with each SDK version update

**Option B: Build a custom pipeline using the same underlying tools**
- Use `ClangSharpPInvokeGenerator` directly (it's a standalone .NET tool, works on Linux)
  to generate C# from your headers
- Use the Emitter code (or a subset of it) to compile the C# into a winmd
- Skip the parts you don't need (MIDL, lib scanning, cross-arch merging)
- This is essentially what bindscrape aims to do

**Option C: Skip winmd entirely**
- Use `ClangSharpPInvokeGenerator` to generate C# bindings directly
- Or use libClang bindings in your target language (e.g., Rust's `bindgen` crate) to
  parse headers directly without the winmd intermediate format
- The winmd format is most useful when you need a single metadata file consumed by
  multiple language projections; if you only target one language, direct generation may
  be simpler

---

## Key Insight

The critical design decision is using **libClang (LLVM)** as a real C/C++ parser rather
than writing a custom parser or relying solely on regex. This provides:

- Correct handling of complex C constructs (nested structs, bitfields, function pointers,
  COM vtables, preprocessor-resolved types)
- Architecture-aware parsing (different struct layouts per architecture)
- Reliable type resolution across `#include` chains

The regex-based ConstantsScraper is a targeted supplement only for `#define` macros, which
are inherently outside the reach of any AST-based tool.
