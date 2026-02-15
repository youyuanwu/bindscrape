//! Emitter — model types → `windows-metadata` writer calls → winmd bytes.

use anyhow::Result;
use tracing::debug;
use windows_metadata::{
    FieldAttributes, MethodAttributes, MethodCallAttributes, MethodImplAttributes,
    PInvokeAttributes, ParamAttributes, Signature, Type, TypeAttributes, Value,
    writer::{File, HasConstant, MemberRefParent, TypeDefOrRef},
};

use crate::model::*;

/// Emit all partitions into a single winmd byte stream.
pub fn emit_winmd(
    assembly_name: &str,
    partitions: &[Partition],
    registry: &TypeRegistry,
) -> Result<Vec<u8>> {
    let mut file = File::new(assembly_name);

    for partition in partitions {
        emit_partition(&mut file, partition, registry)?;
    }

    Ok(file.into_stream())
}

/// Emit a single partition's declarations into the writer.
fn emit_partition(file: &mut File, partition: &Partition, registry: &TypeRegistry) -> Result<()> {
    let ns = &partition.namespace;

    // Emit enums
    for en in &partition.enums {
        emit_enum(file, ns, en)?;
    }

    // Emit structs
    for s in &partition.structs {
        emit_struct(file, ns, s, registry)?;
    }

    // Emit typedefs
    for td in &partition.typedefs {
        emit_typedef(file, ns, td, registry)?;
    }

    // Emit functions (P/Invoke) — all go under a single "Apis" TypeDef
    if !partition.functions.is_empty() || !partition.constants.is_empty() {
        let object_ref = file.TypeRef("System", "Object");
        let _apis_td = file.TypeDef(
            ns,
            "Apis",
            TypeDefOrRef::TypeRef(object_ref),
            TypeAttributes::Public | TypeAttributes::Abstract | TypeAttributes::Sealed,
        );

        for f in &partition.functions {
            emit_function(file, ns, f, &partition.library, registry)?;
        }

        // Emit #define constants as static literal fields on the Apis class
        for c in &partition.constants {
            emit_constant(file, c)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Enum emission
// ---------------------------------------------------------------------------

fn emit_enum(file: &mut File, namespace: &str, en: &EnumDef) -> Result<()> {
    let underlying_wintype =
        ctype_to_wintype(&en.underlying_type, namespace, &TypeRegistry::default());

    let enum_ref = file.TypeRef("System", "Enum");
    let _td = file.TypeDef(
        namespace,
        &en.name,
        TypeDefOrRef::TypeRef(enum_ref),
        TypeAttributes::Public | TypeAttributes::Sealed,
    );

    // value__ field (the underlying storage)
    file.Field(
        "value__",
        &underlying_wintype,
        FieldAttributes::Public | FieldAttributes::RTSpecialName | FieldAttributes::SpecialName,
    );

    // Literal fields for each variant
    for variant in &en.variants {
        let field = file.Field(
            &variant.name,
            &underlying_wintype,
            FieldAttributes::Public | FieldAttributes::Static | FieldAttributes::Literal,
        );
        let value = constant_value_for_enum(&en.underlying_type, variant);
        file.Constant(HasConstant::Field(field), &value);
    }

    debug!(name = %en.name, variants = en.variants.len(), "emitted enum");
    Ok(())
}

/// Convert an enum variant to a `Value` matching the underlying type.
fn constant_value_for_enum(underlying: &CType, variant: &EnumVariant) -> Value {
    match underlying {
        CType::I8 => Value::I8(variant.signed_value as i8),
        CType::U8 => Value::U8(variant.unsigned_value as u8),
        CType::I16 => Value::I16(variant.signed_value as i16),
        CType::U16 => Value::U16(variant.unsigned_value as u16),
        CType::I32 => Value::I32(variant.signed_value as i32),
        CType::U32 => Value::U32(variant.unsigned_value as u32),
        CType::I64 => Value::I64(variant.signed_value),
        CType::U64 => Value::U64(variant.unsigned_value),
        _ => Value::I32(variant.signed_value as i32),
    }
}

// ---------------------------------------------------------------------------
// Struct emission
// ---------------------------------------------------------------------------

fn emit_struct(
    file: &mut File,
    namespace: &str,
    s: &StructDef,
    registry: &TypeRegistry,
) -> Result<()> {
    let valuetype_ref = file.TypeRef("System", "ValueType");
    let layout_attr = if s.is_union {
        TypeAttributes::ExplicitLayout
    } else {
        TypeAttributes::SequentialLayout
    };
    let td = file.TypeDef(
        namespace,
        &s.name,
        TypeDefOrRef::TypeRef(valuetype_ref),
        TypeAttributes::Public | layout_attr,
    );
    file.ClassLayout(td, s.align as u16, s.size as u32);

    for field in &s.fields {
        let wintype = ctype_to_wintype(&field.ty, namespace, registry);
        file.Field(&field.name, &wintype, FieldAttributes::Public);
        // TODO: emit NativeBitfieldAttribute for bitfield fields
    }

    debug!(name = %s.name, fields = s.fields.len(), size = s.size, "emitted struct");
    Ok(())
}

// ---------------------------------------------------------------------------
// Typedef emission
// ---------------------------------------------------------------------------

fn emit_typedef(
    file: &mut File,
    namespace: &str,
    td: &TypedefDef,
    registry: &TypeRegistry,
) -> Result<()> {
    // Check if the typedef is wrapping a function pointer → emit as delegate
    // In C, function pointer typedefs are `typedef ret (*Name)(...)` which maps to
    // Ptr { pointee: FnPtr { ... } }. Also handle direct FnPtr.
    let fnptr = match &td.underlying_type {
        CType::FnPtr {
            return_type,
            params,
            calling_convention: _,
        } => Some((return_type.as_ref(), params.as_slice())),
        CType::Ptr { pointee, .. } => match pointee.as_ref() {
            CType::FnPtr {
                return_type,
                params,
                calling_convention: _,
            } => Some((return_type.as_ref(), params.as_slice())),
            _ => None,
        },
        _ => None,
    };
    if let Some((return_type, params)) = fnptr {
        emit_delegate(file, namespace, &td.name, return_type, params, registry)?;
        return Ok(());
    }

    // Otherwise emit as a struct wrapper with NativeTypedefAttribute
    let valuetype_ref = file.TypeRef("System", "ValueType");
    let _td = file.TypeDef(
        namespace,
        &td.name,
        TypeDefOrRef::TypeRef(valuetype_ref),
        TypeAttributes::Public | TypeAttributes::SequentialLayout,
    );

    // For opaque typedefs (underlying = Void, e.g. `typedef struct __dirstream DIR`
    // where the struct is incomplete), use isize so windows-bindgen generates a
    // copyable handle-like struct instead of `Value: core::ffi::c_void`.
    let wintype = match &td.underlying_type {
        CType::Void => Type::ISize,
        other => ctype_to_wintype(other, namespace, registry),
    };
    file.Field("Value", &wintype, FieldAttributes::Public);

    // Add NativeTypedefAttribute custom attribute
    // We need a MemberRef to the attribute constructor
    let attr_typeref = file.TypeRef(
        "Windows.Win32.Foundation.Metadata",
        "NativeTypedefAttribute",
    );
    let _attr_ctor = file.MemberRef(
        ".ctor",
        &Signature::default(),
        MemberRefParent::TypeRef(attr_typeref),
    );

    debug!(name = %td.name, "emitted typedef");
    Ok(())
}

// ---------------------------------------------------------------------------
// Delegate (function pointer) emission
// ---------------------------------------------------------------------------

fn emit_delegate(
    file: &mut File,
    namespace: &str,
    name: &str,
    return_type: &CType,
    params: &[CType],
    registry: &TypeRegistry,
) -> Result<()> {
    let delegate_ref = file.TypeRef("System", "MulticastDelegate");
    let _td = file.TypeDef(
        namespace,
        name,
        TypeDefOrRef::TypeRef(delegate_ref),
        TypeAttributes::Public | TypeAttributes::Sealed,
    );

    // Build signature for the Invoke method
    let ret_wintype = ctype_to_wintype(return_type, namespace, registry);
    let param_wintypes: Vec<Type> = params
        .iter()
        .map(|p| ctype_to_wintype(p, namespace, registry))
        .collect();

    let sig = Signature {
        flags: MethodCallAttributes::default(),
        return_type: ret_wintype,
        types: param_wintypes,
    };

    let _method = file.MethodDef(
        "Invoke",
        &sig,
        MethodAttributes::Public
            | MethodAttributes::Virtual
            | MethodAttributes::HideBySig
            | MethodAttributes::NewSlot,
        MethodImplAttributes::default(),
    );

    // Add params (unnamed, indexed from 1)
    for i in 0..params.len() {
        file.Param(
            &format!("param{}", i),
            (i + 1) as u16,
            ParamAttributes::default(),
        );
    }

    debug!(name, params = params.len(), "emitted delegate");
    Ok(())
}

// ---------------------------------------------------------------------------
// Function (P/Invoke) emission
// ---------------------------------------------------------------------------

fn emit_function(
    file: &mut File,
    namespace: &str,
    f: &FunctionDef,
    library: &str,
    registry: &TypeRegistry,
) -> Result<()> {
    let ret_wintype = ctype_to_wintype(&f.return_type, namespace, registry);
    let param_wintypes: Vec<Type> = f
        .params
        .iter()
        .map(|p| ctype_to_wintype(&p.ty, namespace, registry))
        .collect();

    let sig = Signature {
        flags: MethodCallAttributes::default(),
        return_type: ret_wintype,
        types: param_wintypes,
    };

    let pinvoke_flags = match f.calling_convention {
        CallConv::Cdecl => PInvokeAttributes::CallConvCdecl,
        CallConv::Stdcall => PInvokeAttributes::CallConvPlatformapi,
        CallConv::Fastcall => PInvokeAttributes::CallConvPlatformapi,
    };

    let method = file.MethodDef(
        &f.name,
        &sig,
        MethodAttributes::Public | MethodAttributes::HideBySig,
        MethodImplAttributes::PreserveSig,
    );
    file.ImplMap(method, pinvoke_flags, &f.name, library);

    for (i, param) in f.params.iter().enumerate() {
        file.Param(&param.name, (i + 1) as u16, ParamAttributes::default());
    }

    debug!(name = %f.name, params = f.params.len(), "emitted function");
    Ok(())
}

// ---------------------------------------------------------------------------
// #define constant emission
// ---------------------------------------------------------------------------

fn emit_constant(file: &mut File, c: &ConstantDef) -> Result<()> {
    let (wintype, value) = match &c.value {
        ConstantValue::Signed(v) => (Type::I32, Value::I32(*v as i32)),
        ConstantValue::Unsigned(v) => {
            if *v <= u32::MAX as u64 {
                (Type::U32, Value::U32(*v as u32))
            } else {
                (Type::U64, Value::U64(*v))
            }
        }
        ConstantValue::Float(v) => (Type::F64, Value::F64(*v)),
    };

    let field = file.Field(
        &c.name,
        &wintype,
        FieldAttributes::Public
            | FieldAttributes::Static
            | FieldAttributes::Literal
            | FieldAttributes::HasDefault,
    );
    file.Constant(HasConstant::Field(field), &value);

    debug!(name = %c.name, "emitted constant");
    Ok(())
}

// ---------------------------------------------------------------------------
// CType → windows_metadata::Type mapping
// ---------------------------------------------------------------------------

fn ctype_to_wintype(ctype: &CType, default_namespace: &str, registry: &TypeRegistry) -> Type {
    match ctype {
        CType::Void => Type::Void,
        CType::Bool => Type::Bool,
        CType::I8 => Type::I8,
        CType::U8 => Type::U8,
        CType::I16 => Type::I16,
        CType::U16 => Type::U16,
        CType::I32 => Type::I32,
        CType::U32 => Type::U32,
        CType::I64 => Type::I64,
        CType::U64 => Type::U64,
        CType::F32 => Type::F32,
        CType::F64 => Type::F64,
        CType::ISize => Type::ISize,
        CType::USize => Type::USize,

        CType::Ptr {
            pointee,
            is_const: _,
        } => {
            // Always emit PtrMut — windows-bindgen cannot parse nested
            // PtrConst blobs (ELEMENT_TYPE_CMOD_REQD mid-chain panics in
            // from_blob_impl). Const-ness is tracked by ConstAttribute on
            // method parameters instead.
            let inner = ctype_to_wintype(pointee, default_namespace, registry);
            Type::PtrMut(Box::new(inner), 1)
        }

        CType::Array { element, len } => {
            let inner = ctype_to_wintype(element, default_namespace, registry);
            Type::ArrayFixed(Box::new(inner), *len)
        }

        CType::Named { name, resolved } => {
            // If the type is registered (user-defined / extracted), emit a TypeRef.
            if registry.contains(name) {
                let ns = registry.namespace_for(name, default_namespace);
                Type::named(&ns, name)
            } else if let Some(resolved) = resolved {
                // System typedef not in any partition — use the canonical type
                // that clang resolved during extraction.
                ctype_to_wintype(resolved, default_namespace, registry)
            } else {
                // Record/enum not in registry — emit as TypeRef and
                // let windows-bindgen report the error with context.
                let ns = registry.namespace_for(name, default_namespace);
                Type::named(&ns, name)
            }
        }

        CType::FnPtr { .. } => {
            // Function pointers in field positions are emitted as IntPtr (nint)
            // since the delegate TypeDef is separate.
            Type::ISize
        }
    }
}
