//! Extraction — clang `Entity`/`Type` → intermediate model types.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use std::collections::HashSet;

use clang::{
    CallingConvention, Entity, EntityKind, Index, Type as ClangType, TypeKind,
    sonar::{self, Declaration, DefinitionValue},
};
use tracing::{debug, trace, warn};

use crate::config::{self, PartitionConfig};
use crate::model::*;

/// Extract all declarations from a single partition into model types.
pub fn extract_partition(
    index: &Index,
    partition: &PartitionConfig,
    base_dir: &Path,
    include_paths: &[PathBuf],
    namespace_overrides: &std::collections::HashMap<String, String>,
) -> Result<Partition> {
    let _ = namespace_overrides; // reserved for future per-API namespace overrides
    let header_path = partition.wrapper_header(base_dir, include_paths);
    debug!(header = %header_path.display(), namespace = %partition.namespace, "parsing partition");

    // Build clang arguments: user-specified args + -I flags from include_paths
    let mut all_args: Vec<String> = partition.clang_args.clone();
    for inc in include_paths {
        let flag = format!("-I{}", inc.display());
        if !all_args.contains(&flag) {
            all_args.push(flag);
        }
    }

    let tu = index
        .parser(header_path.to_str().unwrap())
        .arguments(&all_args.iter().map(|s| s.as_str()).collect::<Vec<_>>())
        .detailed_preprocessing_record(true)
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {:?}", header_path.display(), e))?;

    // Resolve traverse files through include_paths so relative names work
    let resolved_traverse: Vec<PathBuf> = partition
        .traverse_files()
        .iter()
        .map(|t| config::resolve_header(t, base_dir, include_paths))
        .collect();
    let entities = tu.get_entity().get_children();

    let in_scope = |e: &Entity| should_emit(e, &resolved_traverse, base_dir);

    let structs = collect_structs(&entities, &in_scope);
    let (enums, anon_enum_constants) = collect_enums(&entities, &in_scope);
    let functions = collect_functions(&entities, &in_scope);
    let typedefs = collect_typedefs(&entities, &in_scope);
    let mut constants = collect_constants(&entities, &in_scope);

    // Merge in constants extracted from anonymous enums
    constants.extend(anon_enum_constants);

    tracing::info!(
        namespace = %partition.namespace,
        structs = structs.len(),
        enums = enums.len(),
        functions = functions.len(),
        typedefs = typedefs.len(),
        constants = constants.len(),
        "partition extraction complete"
    );

    Ok(Partition {
        namespace: partition.namespace.clone(),
        library: partition.library.clone(),
        structs,
        enums,
        functions,
        typedefs,
        constants,
    })
}

// ---------------------------------------------------------------------------
// Collection helpers — one per declaration kind
// ---------------------------------------------------------------------------

/// Collect structs via sonar, then run a supplemental pass for StructDecl
/// entities that sonar missed (e.g. structs that only have a pointer typedef).
fn collect_structs(entities: &[Entity], in_scope: &impl Fn(&Entity) -> bool) -> Vec<StructDef> {
    let mut structs = Vec::new();
    let mut seen = HashSet::new();

    // Primary: sonar-discovered structs (via typedef patterns)
    for decl in sonar::find_structs(entities.to_vec()) {
        if !in_scope(&decl.entity) {
            continue;
        }
        seen.insert(decl.name.clone());
        match extract_struct(&decl) {
            Ok((s, nested)) => {
                debug!(name = %s.name, fields = s.fields.len(), size = s.size, "extracted struct");
                for ns in nested {
                    seen.insert(ns.name.clone());
                    debug!(name = %ns.name, fields = ns.fields.len(), "  nested anonymous type");
                    structs.push(ns);
                }
                structs.push(s);
            }
            Err(e) => warn!(name = %decl.name, err = %e, "skipping struct"),
        }
    }

    // Supplemental: StructDecl/UnionDecl entities with full definitions that
    // sonar missed (e.g. `struct gzFile_s` which only has a pointer typedef,
    // or any union — sonar has no find_unions).
    for entity in entities {
        let is_union = match entity.get_kind() {
            EntityKind::StructDecl => false,
            EntityKind::UnionDecl => true,
            _ => continue,
        };
        if !in_scope(entity) {
            continue;
        }
        let name = match entity.get_name() {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        if seen.contains(&name) || !entity.is_definition() {
            continue;
        }
        seen.insert(name.clone());
        match extract_struct_from_entity(entity, &name, is_union) {
            Ok((s, nested)) => {
                let kind = if is_union { "union" } else { "struct" };
                debug!(name = %s.name, fields = s.fields.len(), size = s.size, "extracted {kind} (supplemental)");
                for ns in nested {
                    seen.insert(ns.name.clone());
                    debug!(name = %ns.name, fields = ns.fields.len(), "  nested anonymous type");
                    structs.push(ns);
                }
                structs.push(s);
            }
            Err(e) => warn!(name = %name, err = %e, "skipping struct/union"),
        }
    }

    structs
}

/// Collect enums via sonar.
fn collect_enums(
    entities: &[Entity],
    in_scope: &impl Fn(&Entity) -> bool,
) -> (Vec<EnumDef>, Vec<ConstantDef>) {
    let mut enums = Vec::new();
    let mut anon_constants = Vec::new();
    for decl in sonar::find_enums(entities.to_vec()) {
        if !in_scope(&decl.entity) {
            continue;
        }
        // Detect anonymous enums (e.g. `enum { DT_UNKNOWN = 0, ... }`).
        // clang gives them names like "enum (unnamed at /usr/include/dirent.h:97:1)".
        // These are just collections of integer constants in C — emit their
        // variants as standalone ConstantDef entries instead of a named enum.
        if decl.entity.is_anonymous() || decl.name.contains("(unnamed") {
            match extract_enum(&decl) {
                Ok(en) => {
                    debug!(
                        name = %decl.name,
                        variants = en.variants.len(),
                        "anonymous enum → emitting variants as constants"
                    );
                    for variant in en.variants {
                        let value = if variant.signed_value < 0 {
                            ConstantValue::Signed(variant.signed_value)
                        } else {
                            ConstantValue::Unsigned(variant.unsigned_value)
                        };
                        anon_constants.push(ConstantDef {
                            name: variant.name,
                            value,
                        });
                    }
                }
                Err(e) => warn!(name = %decl.name, err = %e, "skipping anonymous enum"),
            }
            continue;
        }
        match extract_enum(&decl) {
            Ok(en) => {
                debug!(name = %en.name, variants = en.variants.len(), "extracted enum");
                enums.push(en);
            }
            Err(e) => warn!(name = %decl.name, err = %e, "skipping enum"),
        }
    }
    (enums, anon_constants)
}

/// Collect functions via sonar.
fn collect_functions(entities: &[Entity], in_scope: &impl Fn(&Entity) -> bool) -> Vec<FunctionDef> {
    let mut functions = Vec::new();
    let mut seen = HashSet::new();
    for decl in sonar::find_functions(entities.to_vec()) {
        if !in_scope(&decl.entity) {
            continue;
        }
        // Skip variadic functions — P/Invoke metadata cannot represent `...`
        if decl.entity.is_variadic() {
            warn!(name = %decl.name, "skipping variadic function");
            continue;
        }
        match extract_function(&decl) {
            Ok(f) => {
                // Deduplicate by name — glibc __REDIRECT macros can produce
                // multiple declarations of the same function (e.g. lockf / lockf64).
                if !seen.insert(f.name.clone()) {
                    trace!(name = %f.name, "skipping duplicate function");
                    continue;
                }
                debug!(name = %f.name, params = f.params.len(), "extracted function");
                functions.push(f);
            }
            Err(e) => warn!(name = %decl.name, err = %e, "skipping function"),
        }
    }
    functions
}

/// Collect typedefs via custom discovery (not sonar, which drops typedef-to-
/// typedef aliases like `typedef Byte Bytef`).
fn collect_typedefs(entities: &[Entity], in_scope: &impl Fn(&Entity) -> bool) -> Vec<TypedefDef> {
    let mut typedefs = Vec::new();
    let mut seen = HashSet::new();
    for entity in entities {
        if entity.get_kind() != EntityKind::TypedefDecl {
            continue;
        }
        if !in_scope(entity) {
            continue;
        }
        let name = match entity.get_name() {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        if !seen.insert(name.clone()) {
            continue;
        }
        let underlying = match entity.get_typedef_underlying_type() {
            Some(ut) => ut,
            None => continue,
        };
        // Skip trivial struct/enum/union pass-throughs like `typedef struct foo foo;`
        if is_struct_passthrough(&underlying, &name) {
            trace!(name = %name, "skipping struct/enum passthrough typedef");
            continue;
        }
        match extract_typedef_from_entity(entity, &name) {
            Ok(td) => {
                debug!(name = %td.name, "extracted typedef");
                typedefs.push(td);
            }
            Err(e) => warn!(name = %name, err = %e, "skipping typedef"),
        }
    }
    typedefs
}

/// Collect `#define` constants via sonar + supplemental hex parsing.
fn collect_constants(entities: &[Entity], in_scope: &impl Fn(&Entity) -> bool) -> Vec<ConstantDef> {
    let mut constants = Vec::new();
    let mut seen = HashSet::new();

    // Primary: sonar-discovered constants (decimal integers + floats)
    for def in sonar::find_definitions(entities.to_vec()) {
        if !in_scope(&def.entity) {
            continue;
        }
        let value = match def.value {
            DefinitionValue::Integer(negated, val) => {
                if negated {
                    ConstantValue::Signed(-(val as i64))
                } else if val <= i64::MAX as u64 {
                    ConstantValue::Signed(val as i64)
                } else {
                    ConstantValue::Unsigned(val)
                }
            }
            DefinitionValue::Real(val) => ConstantValue::Float(val),
        };
        debug!(name = %def.name, "extracted #define constant");
        seen.insert(def.name.clone());
        constants.push(ConstantDef {
            name: def.name,
            value,
        });
    }

    // Supplemental: hex constants that sonar's u64::from_str misses.
    // sonar only parses decimal; `#define PROT_READ 0x1` is silently skipped.
    for entity in entities {
        if entity.get_kind() != EntityKind::MacroDefinition {
            continue;
        }
        if !in_scope(entity) {
            continue;
        }
        let name = match entity.get_name() {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        if seen.contains(&name) {
            continue;
        }
        if let Some(range) = entity.get_range() {
            let mut tokens: Vec<String> =
                range.tokenize().iter().map(|t| t.get_spelling()).collect();
            // Strip trailing "#" that clang sometimes appends
            if tokens.last().is_some_and(|t| t == "#") {
                tokens.pop();
            }
            let (negated, number) = if tokens.len() == 2 {
                (false, &tokens[1])
            } else if tokens.len() == 3 && tokens[1] == "-" {
                (true, &tokens[2])
            } else {
                continue;
            };
            if let Some(val) = parse_hex_or_suffixed_int(number) {
                let value = if negated {
                    ConstantValue::Signed(-(val as i64))
                } else if val <= i64::MAX as u64 {
                    ConstantValue::Signed(val as i64)
                } else {
                    ConstantValue::Unsigned(val)
                };
                debug!(name = %name, "extracted #define hex constant");
                seen.insert(name.clone());
                constants.push(ConstantDef { name, value });
            }
        }
    }

    constants
}

/// Parse a hex literal (`0x1F`) or a suffixed integer (`1U`, `0x10UL`, etc.)
/// that `u64::from_str` can't handle. Returns None if not parseable.
fn parse_hex_or_suffixed_int(s: &str) -> Option<u64> {
    // Strip trailing integer suffixes: U, L, LL, UL, ULL (case-insensitive)
    let s = s.trim_end_matches(['u', 'U', 'l', 'L']);

    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else if let Some(octal) = s.strip_prefix("0") {
        if octal.is_empty() {
            Some(0) // "0" with suffixes stripped
        } else if octal.chars().all(|c| c.is_ascii_digit()) {
            u64::from_str_radix(octal, 8).ok()
        } else {
            None
        }
    } else {
        // Try decimal (handles cases where suffix stripping exposed a plain decimal)
        s.parse::<u64>().ok()
    }
}

// ---------------------------------------------------------------------------
// Struct extraction
// ---------------------------------------------------------------------------

fn extract_struct(decl: &Declaration) -> Result<(StructDef, Vec<StructDef>)> {
    extract_struct_from_entity(&decl.entity, &decl.name, false)
}

fn extract_struct_from_entity(
    entity: &Entity,
    name: &str,
    is_union: bool,
) -> Result<(StructDef, Vec<StructDef>)> {
    let ty = entity.get_type().context("struct has no type")?;
    let size = ty.get_sizeof().unwrap_or(0);
    let align = ty.get_alignof().unwrap_or(0);

    let mut fields = Vec::new();
    let mut nested_types = Vec::new();
    for child in entity.get_children() {
        if child.get_kind() != EntityKind::FieldDecl {
            continue;
        }
        let field_name = child.get_name().unwrap_or_default();
        let field_type = child.get_type().context("field has no type")?;

        // Check for anonymous record type (unnamed struct/union used as a field type).
        // Clang gives these names like "union (unnamed at file.h:37:5)" which can't
        // be resolved. We extract them as separate TypeDefs with synthetic names.
        let ctype =
            match try_extract_anonymous_field(&field_type, name, &field_name, &mut nested_types) {
                Some(synthetic_name) => CType::Named {
                    name: synthetic_name,
                    resolved: None,
                },
                None => map_clang_type(&field_type)
                    .with_context(|| format!("unsupported type for field '{}'", field_name))?,
            };

        let bitfield_width = if child.is_bit_field() {
            child.get_bit_field_width()
        } else {
            None
        };
        let bitfield_offset = if child.is_bit_field() {
            child.get_offset_of_field().ok()
        } else {
            None
        };

        trace!(field = %field_name, ty = ?ctype, "  field");
        fields.push(FieldDef {
            name: field_name,
            ty: ctype,
            bitfield_width,
            bitfield_offset,
        });
    }

    Ok((
        StructDef {
            name: name.to_string(),
            size,
            align,
            fields,
            is_union,
        },
        nested_types,
    ))
}

/// Try to extract an anonymous record field type as a synthetic named type.
///
/// When a struct/union contains a field whose type is an anonymous record
/// (e.g. `union { int a; float b; } field;`), clang gives it a non-portable
/// name like `"union (unnamed at file.h:37:5)"`. This function detects that
/// case, recursively extracts the anonymous record as a separate `StructDef`
/// with a synthetic name `ParentName_FieldName`, and returns the synthetic
/// name so the caller can reference it.
fn try_extract_anonymous_field(
    field_type: &ClangType,
    parent_name: &str,
    field_name: &str,
    nested_types: &mut Vec<StructDef>,
) -> Option<String> {
    let canonical = field_type.get_canonical_type();
    if canonical.get_kind() != TypeKind::Record {
        return None;
    }
    let decl = canonical.get_declaration()?;
    if !decl.is_anonymous() {
        return None;
    }
    let is_nested_union = decl.get_kind() == EntityKind::UnionDecl;
    let synthetic_name = format!("{}_{}", parent_name, field_name);

    match extract_struct_from_entity(&decl, &synthetic_name, is_nested_union) {
        Ok((nested, mut more)) => {
            let kind = if is_nested_union { "union" } else { "struct" };
            debug!(
                parent = %parent_name,
                field = %field_name,
                synthetic = %synthetic_name,
                "extracted anonymous {kind} as synthetic type"
            );
            nested_types.push(nested);
            nested_types.append(&mut more); // handle deeply nested anonymous types
            Some(synthetic_name)
        }
        Err(e) => {
            warn!(
                parent = %parent_name,
                field = %field_name,
                err = %e,
                "failed to extract anonymous nested type"
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Enum extraction
// ---------------------------------------------------------------------------

fn extract_enum(decl: &Declaration) -> Result<EnumDef> {
    let underlying = decl
        .entity
        .get_enum_underlying_type()
        .context("enum has no underlying type")?;
    let underlying_ctype = map_clang_type(&underlying).unwrap_or(CType::I32); // fallback to i32

    let mut variants = Vec::new();
    for child in decl.entity.get_children() {
        if child.get_kind() != EntityKind::EnumConstantDecl {
            continue;
        }
        let name = child.get_name().unwrap_or_default();
        let (signed, unsigned) = child.get_enum_constant_value().unwrap_or((0, 0));
        variants.push(EnumVariant {
            name,
            signed_value: signed,
            unsigned_value: unsigned,
        });
    }

    Ok(EnumDef {
        name: decl.name.clone(),
        underlying_type: underlying_ctype,
        variants,
    })
}

// ---------------------------------------------------------------------------
// Function extraction
// ---------------------------------------------------------------------------

fn extract_function(decl: &Declaration) -> Result<FunctionDef> {
    let fn_type = decl.entity.get_type().context("function has no type")?;

    let ret_type = fn_type
        .get_result_type()
        .context("function has no return type")?;
    let return_ctype = map_clang_type(&ret_type).unwrap_or(CType::Void);

    let calling_convention = fn_type
        .get_calling_convention()
        .map(map_calling_convention)
        .unwrap_or(CallConv::Cdecl);

    let args = decl.entity.get_arguments().unwrap_or_default();
    let arg_types = fn_type.get_argument_types().unwrap_or_default();

    let mut params = Vec::new();
    for (i, arg_entity) in args.iter().enumerate() {
        let name = arg_entity
            .get_name()
            .unwrap_or_else(|| format!("param{}", i));
        let ty = if i < arg_types.len() {
            map_clang_type(&arg_types[i]).unwrap_or(CType::Void)
        } else {
            CType::Void
        };
        // C array parameters decay to pointers (e.g. `const struct timespec t[2]` → `*timespec`).
        // We must do this here because ELEMENT_TYPE_ARRAY blobs in method signatures can confuse
        // windows-bindgen's reader which doesn't consume all ArrayShape fields.
        let ty = match ty {
            CType::Array { element, .. } => CType::Ptr {
                pointee: element,
                is_const: false,
            },
            other => other,
        };
        params.push(ParamDef { name, ty });
    }

    Ok(FunctionDef {
        name: decl.name.clone(),
        return_type: return_ctype,
        params,
        calling_convention,
    })
}

// ---------------------------------------------------------------------------
// Typedef extraction
// ---------------------------------------------------------------------------

fn extract_typedef_from_entity(entity: &Entity, name: &str) -> Result<TypedefDef> {
    let underlying = entity
        .get_typedef_underlying_type()
        .context("typedef has no underlying type")?;
    let ctype = map_clang_type(&underlying).unwrap_or(CType::Void);
    trace!(name = %name, ty = ?ctype, "typedef underlying type");

    Ok(TypedefDef {
        name: name.to_string(),
        underlying_type: ctype,
    })
}

// ---------------------------------------------------------------------------
// Type mapping: clang TypeKind → CType
// ---------------------------------------------------------------------------

fn map_clang_type(ty: &ClangType) -> Result<CType> {
    match ty.get_kind() {
        TypeKind::Void => Ok(CType::Void),
        TypeKind::Bool => Ok(CType::Bool),
        TypeKind::CharS | TypeKind::SChar => Ok(CType::I8),
        TypeKind::CharU | TypeKind::UChar => Ok(CType::U8),
        TypeKind::Short => Ok(CType::I16),
        TypeKind::UShort => Ok(CType::U16),
        TypeKind::Int => Ok(CType::I32),
        TypeKind::UInt => Ok(CType::U32),
        // C `long` is 64-bit on Linux x86-64 (LP64 ABI)
        TypeKind::Long => Ok(CType::I64),
        TypeKind::ULong => Ok(CType::U64),
        TypeKind::LongLong => Ok(CType::I64),
        TypeKind::ULongLong => Ok(CType::U64),
        TypeKind::Float => Ok(CType::F32),
        TypeKind::Double => Ok(CType::F64),

        TypeKind::Pointer => {
            let pointee = ty
                .get_pointee_type()
                .context("pointer has no pointee type")?;
            let is_const = pointee.is_const_qualified();
            let inner = map_clang_type(&pointee)?;
            Ok(CType::Ptr {
                pointee: Box::new(inner),
                is_const,
            })
        }

        TypeKind::ConstantArray => {
            let elem = ty.get_element_type().context("array has no element type")?;
            let len = ty.get_size().unwrap_or(0);
            let inner = map_clang_type(&elem)?;
            Ok(CType::Array {
                element: Box::new(inner),
                len,
            })
        }

        TypeKind::IncompleteArray => {
            // Treat as pointer
            let elem = ty
                .get_element_type()
                .context("incomplete array has no element type")?;
            let inner = map_clang_type(&elem)?;
            Ok(CType::Ptr {
                pointee: Box::new(inner),
                is_const: false,
            })
        }

        TypeKind::Elaborated => {
            let inner = ty
                .get_elaborated_type()
                .context("elaborated type has no inner type")?;
            map_clang_type(&inner)
        }

        TypeKind::Typedef => {
            let decl = ty.get_declaration();
            if let Some(decl) = decl {
                let name = decl.get_name().unwrap_or_default();
                if !name.is_empty() {
                    // va_list is a compiler built-in with no portable canonical type
                    if matches!(
                        name.as_str(),
                        "va_list" | "__builtin_va_list" | "__gnuc_va_list"
                    ) {
                        return Ok(CType::Ptr {
                            pointee: Box::new(CType::Void),
                            is_const: false,
                        });
                    }
                    // Keep the name for cross-partition TypeRef resolution,
                    // but also resolve the canonical type as fallback for
                    // system typedefs that won't be in any partition.
                    let canonical = ty.get_canonical_type();
                    let resolved = map_clang_type(&canonical).ok().map(Box::new);
                    return Ok(CType::Named { name, resolved });
                }
            }
            // Unnamed or unresolvable typedef — resolve to canonical primitive
            let canonical = ty.get_canonical_type();
            map_clang_type(&canonical)
        }

        TypeKind::Record => {
            let decl = ty.get_declaration();
            if let Some(decl) = decl
                && let Some(name) = decl.get_name()
            {
                // Check if the type is complete (has a definition, not just forward-declared).
                // Incomplete/opaque types (like `struct internal_state` in zlib) are
                // mapped to Void so that pointers to them become `*mut c_void`.
                if ty.get_sizeof().is_ok() {
                    return Ok(CType::Named {
                        name,
                        resolved: None,
                    });
                } else {
                    debug!(name = %name, "incomplete record type, mapping to Void");
                    return Ok(CType::Void);
                }
            }
            anyhow::bail!("anonymous record type without name")
        }

        TypeKind::Enum => {
            let decl = ty.get_declaration();
            if let Some(decl) = decl
                && let Some(name) = decl.get_name()
            {
                return Ok(CType::Named {
                    name,
                    resolved: None,
                });
            }
            anyhow::bail!("anonymous enum type without name")
        }

        TypeKind::FunctionPrototype => {
            let ret = ty
                .get_result_type()
                .context("function prototype has no return type")?;
            let ret_ctype = map_clang_type(&ret)?;
            let arg_types = ty.get_argument_types().unwrap_or_default();
            let mut params = Vec::new();
            for at in &arg_types {
                params.push(map_clang_type(at)?);
            }
            let cc = ty
                .get_calling_convention()
                .map(map_calling_convention)
                .unwrap_or(CallConv::Cdecl);
            Ok(CType::FnPtr {
                return_type: Box::new(ret_ctype),
                params,
                calling_convention: cc,
            })
        }

        TypeKind::FunctionNoPrototype => {
            // K&R-style function — treat as void() for now
            Ok(CType::FnPtr {
                return_type: Box::new(CType::Void),
                params: vec![],
                calling_convention: CallConv::Cdecl,
            })
        }

        other => {
            anyhow::bail!("unsupported clang TypeKind: {:?}", other)
        }
    }
}

// ---------------------------------------------------------------------------
// Calling convention mapping
// ---------------------------------------------------------------------------

fn map_calling_convention(cc: CallingConvention) -> CallConv {
    match cc {
        CallingConvention::Cdecl => CallConv::Cdecl,
        CallingConvention::Stdcall => CallConv::Stdcall,
        CallingConvention::Fastcall => CallConv::Fastcall,
        // Everything else → Cdecl (platform default)
        _ => CallConv::Cdecl,
    }
}

// ---------------------------------------------------------------------------
// Typedef filtering helpers
// ---------------------------------------------------------------------------

/// Returns true if this typedef is a trivial struct/enum/union pass-through,
/// i.e. `typedef struct foo foo;` or `typedef enum bar bar;`.
/// These are handled by sonar's find_structs/find_enums and should NOT also
/// appear as typedefs.
fn is_struct_passthrough(underlying: &ClangType, typedef_name: &str) -> bool {
    let display = underlying.get_display_name();
    for prefix in &["struct ", "enum ", "union "] {
        if display.starts_with(prefix) && &display[prefix.len()..] == typedef_name {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Source-location filtering (partition traversal)
// ---------------------------------------------------------------------------

fn should_emit(entity: &Entity, traverse_files: &[PathBuf], base_dir: &Path) -> bool {
    should_emit_by_location(entity, traverse_files, base_dir)
}

fn should_emit_by_location(entity: &Entity, traverse_files: &[PathBuf], _base_dir: &Path) -> bool {
    let location = match entity.get_location() {
        Some(loc) => loc,
        None => return false,
    };
    let file_location = location.get_file_location();
    let file = match file_location.file {
        Some(f) => f,
        None => return false,
    };
    let file_path = file.get_path();

    // traverse_files are already resolved to absolute paths by the caller,
    // so we just compare directly (or by suffix for robustness).
    traverse_files
        .iter()
        .any(|tf| file_path == *tf || file_path.ends_with(tf))
}

/// Build a type registry from all partitions' extracted data.
///
/// Typedefs use first-writer-wins: the first partition to register a typedef
/// name owns it. This means a dedicated "types" partition should come first
/// in the TOML so it claims shared types like `uid_t`, `pid_t`, etc. before
/// other partitions can. Structs and enums still use last-writer-wins (they
/// rarely overlap across partitions).
pub fn build_type_registry(
    partitions: &[Partition],
    namespace_overrides: &std::collections::HashMap<String, String>,
) -> TypeRegistry {
    let mut registry = TypeRegistry::default();
    for partition in partitions {
        for s in &partition.structs {
            let ns = namespace_overrides
                .get(&s.name)
                .unwrap_or(&partition.namespace);
            registry.register(&s.name, ns);
        }
        for e in &partition.enums {
            let ns = namespace_overrides
                .get(&e.name)
                .unwrap_or(&partition.namespace);
            registry.register(&e.name, ns);
        }
        for td in &partition.typedefs {
            // First-writer-wins for typedefs: if already registered by an
            // earlier partition (e.g. a shared types partition), skip.
            if registry.contains(&td.name) {
                continue;
            }
            let ns = namespace_overrides
                .get(&td.name)
                .unwrap_or(&partition.namespace);
            registry.register(&td.name, ns);
        }
    }
    registry
}
