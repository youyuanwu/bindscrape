//! Intermediate model types — the bridge between clang extraction and winmd emission.
//!
//! These types are clang-independent and winmd-independent, making both the extractor
//! and emitter easier to test in isolation.

use std::collections::HashMap;

/// A fully extracted partition ready for winmd emission.
#[derive(Debug)]
pub struct Partition {
    pub namespace: String,
    pub library: String,
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub functions: Vec<FunctionDef>,
    pub typedefs: Vec<TypedefDef>,
    pub constants: Vec<ConstantDef>,
}

/// A C struct or union definition.
#[derive(Debug)]
pub struct StructDef {
    pub name: String,
    pub size: usize,
    pub align: usize,
    pub fields: Vec<FieldDef>,
    /// True if this is a C `union` (all fields at offset 0).
    pub is_union: bool,
}

/// A single struct field.
#[derive(Debug)]
pub struct FieldDef {
    pub name: String,
    pub ty: CType,
    /// If this is a bitfield, the width in bits.
    pub bitfield_width: Option<usize>,
    /// Bit offset of a bitfield within the struct (from clang).
    pub bitfield_offset: Option<usize>,
}

/// A C enum definition.
#[derive(Debug)]
pub struct EnumDef {
    pub name: String,
    /// The underlying integer type (e.g. `CType::U32`).
    pub underlying_type: CType,
    pub variants: Vec<EnumVariant>,
}

/// A single enum variant.
#[derive(Debug)]
pub struct EnumVariant {
    pub name: String,
    /// Value as (signed, unsigned) pair — from clang.
    pub signed_value: i64,
    pub unsigned_value: u64,
}

/// A C function declaration.
#[derive(Debug)]
pub struct FunctionDef {
    pub name: String,
    pub return_type: CType,
    pub params: Vec<ParamDef>,
    pub calling_convention: CallConv,
}

/// A function parameter.
#[derive(Debug)]
pub struct ParamDef {
    pub name: String,
    pub ty: CType,
}

/// A C typedef.
#[derive(Debug)]
pub struct TypedefDef {
    pub name: String,
    pub underlying_type: CType,
}

/// A `#define` integer constant.
#[derive(Debug)]
pub struct ConstantDef {
    pub name: String,
    pub value: ConstantValue,
}

/// Value of a `#define` constant.
#[derive(Debug, Clone)]
pub enum ConstantValue {
    Signed(i64),
    Unsigned(u64),
    Float(f64),
}

/// Calling convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallConv {
    /// Platform default (cdecl on most platforms).
    Cdecl,
    /// stdcall (Windows x86).
    Stdcall,
    /// Fastcall.
    Fastcall,
}

/// A C type — our intermediate representation.
///
/// Maps closely to both clang's `TypeKind` and ECMA-335's `Type` enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CType {
    Void,
    Bool,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    ISize,
    USize,
    /// Pointer to a type. `is_const` indicates `const T*`.
    Ptr {
        pointee: Box<CType>,
        is_const: bool,
    },
    /// Fixed-size array: `T[N]`.
    Array {
        element: Box<CType>,
        len: usize,
    },
    /// A named type reference (struct, enum, typedef in another namespace).
    /// For typedefs, `resolved` holds the canonical primitive type from clang,
    /// used as fallback when the name isn't in the TypeRegistry.
    Named {
        name: String,
        /// Canonical type resolved by clang. `None` for records/enums
        /// (they must be in the registry). `Some` for typedefs so we can
        /// fall back to the primitive when the typedef isn't extracted.
        resolved: Option<Box<CType>>,
    },
    /// A function pointer type.
    FnPtr {
        return_type: Box<CType>,
        params: Vec<CType>,
        calling_convention: CallConv,
    },
}

/// Global type registry — tracks which namespace each named type lives in.
///
/// Built during extraction by scanning all partitions, then used during
/// emission to resolve `CType::Named` references to the correct namespace.
#[derive(Debug, Default)]
pub struct TypeRegistry {
    /// Maps type name → namespace.
    pub types: HashMap<String, String>,
}

impl TypeRegistry {
    pub fn register(&mut self, name: &str, namespace: &str) {
        self.types.insert(name.to_string(), namespace.to_string());
    }

    /// Returns true if the type name is registered (i.e. was extracted from
    /// a partition, as opposed to being a system/platform typedef).
    pub fn contains(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }

    /// Look up the namespace for a named type. Returns the type's own
    /// namespace if registered, otherwise falls back to `default_namespace`.
    pub fn namespace_for(&self, name: &str, default_namespace: &str) -> String {
        self.types
            .get(name)
            .cloned()
            .unwrap_or_else(|| default_namespace.to_string())
    }
}
