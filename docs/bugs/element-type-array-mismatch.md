# ELEMENT_TYPE_ARRAY Blob Mismatch Between windows-metadata Writer and windows-bindgen Reader

## Summary

`windows-metadata` (writer) and `windows-bindgen` (reader) disagree on the
`ELEMENT_TYPE_ARRAY` blob encoding for the ECMA-335 `ArrayShape` (II.23.2.13).
The writer emits **4** compressed integers; the reader only consumes **3**,
leaving a stray `0x00` byte in the blob that causes a panic on the next type
decode.

## Affected Versions

- **Writer**: `windows-metadata` 0.59.0 (`src/writer/file/mod.rs`)
- **Reader**: `windows-bindgen` 0.66.0 (`src/types/mod.rs`)

## Trigger

Any C function with a fixed-size array parameter:

```c
// <sys/stat.h>
extern int futimens(int __fd, const struct timespec __times[2]);
```

bindscrape maps this to `Type::ArrayFixed(timespec, 2)`, which the writer
encodes as `ELEMENT_TYPE_ARRAY`.

## Root Cause

### ECMA-335 II.23.2.13 ArrayShape

```
ArrayShape ::= Rank NumSizes Size* NumLoBounds LoBound*
```

### Writer (`windows-metadata` 0.59.0, `writer/file/mod.rs:409-415`)

```rust
Type::ArrayFixed(ty, len) => {
    buffer.push(ELEMENT_TYPE_ARRAY);
    self.Type(ty, buffer);
    buffer.write_compressed(1);     // Rank
    buffer.write_compressed(1);     // NumSizes
    buffer.write_compressed(*len);  // Size[0]  (e.g. 2)
    buffer.write_compressed(0);     // NumLoBounds
}
```

Emits 4 compressed integers after the element type — correct per spec.

### Reader (`windows-bindgen` 0.66.0, `types/mod.rs:310-316`)

```rust
ELEMENT_TYPE_ARRAY => {
    let kind = Self::from_blob(blob, enclosing, generics);
    let _rank = blob.read_usize();   // reads Rank (1)        ✓
    let _count = blob.read_usize();  // reads NumSizes (1)    ✗ treats as "count"
    let bounds = blob.read_usize();  // reads Size[0] (2)     ✗ treats as "bounds"
    Self::ArrayFixed(Box::new(kind), bounds)
    //                                 ^^^ NumLoBounds (0) left unread!
}
```

Reads only 3 compressed integers. The 4th (`NumLoBounds = 0x00`) remains in
the blob.

### Panic

When processing the *next* parameter after the array, `from_blob_impl` reads
that leftover `0x00` as a `CorElementType` code. `0` is not a valid element
type, so it hits:

```rust
// types/mod.rs — cpp_fn.rs:294 call chain
_ => panic!("{code}")   // panics with message "0"
```

## Workaround (in bindscrape)

C array parameters always decay to pointers (C11 §6.7.6.3p7). bindscrape
applies this decay at extraction time, avoiding `Type::ArrayFixed` entirely
for function parameters:

```rust
// extract.rs — extract_function()
let ty = match ty {
    CType::Array { element, .. } => CType::Ptr {
        pointee: element,
        is_const: false,
    },
    other => other,
};
```

This is semantically correct — `const struct timespec t[2]` as a function
parameter is identical to `const struct timespec *t`.

## Proper Fix

Either:

1. **Fix the reader** (`windows-bindgen`) to consume all 4 ArrayShape fields
   per ECMA-335 spec, or
2. **Fix the writer** (`windows-metadata`) to use `ELEMENT_TYPE_SZARRAY`
   (single-dimension, no bounds) instead of `ELEMENT_TYPE_ARRAY` when
   `rank == 1` and bounds are trivial — this is what the .NET runtime
   typically does.

## References

- ECMA-335 II.23.2.13 — ArrayShape
- ECMA-335 II.23.2.12 — Type (ELEMENT_TYPE_ARRAY vs ELEMENT_TYPE_SZARRAY)
- `windows-metadata` 0.59.0: `src/writer/file/mod.rs:405-415`
- `windows-bindgen` 0.66.0: `src/types/mod.rs:310-316`
