#!/usr/bin/env python3
"""
Convert OpenAPI schema definitions to Markdown documentation for mdbook.

Reads openapi.json and generates markdown files for specified types,
creating human-readable API reference documentation with field tables,
descriptions, and cross-references.
"""

import json
import re
import sys
from pathlib import Path
from typing import Dict, Any, List, Set, Optional, Tuple

# Types to document (top-level types that will get their own pages)
TYPES_TO_DOCUMENT = [
    "HsmWorkerRequestDto",
    "WorkerResponseJws",
    "DeviceHsmState",
    "OuterRequest",
    "OuterResponse",
    "InnerRequest",
    "InnerResponse",
    "SessionId",
    "Status",
    "OperationId",
]


def load_openapi(path: str) -> Dict[str, Any]:
    """Load OpenAPI specification from JSON file."""
    with open(path) as f:
        return json.load(f)


def extract_schemas(openapi: Dict[str, Any]) -> Dict[str, Any]:
    """Extract schemas from components/schemas."""
    return openapi.get("components", {}).get("schemas", {})


def is_wrapper_type(name: str) -> bool:
    """Check if type is a TypedJws_* or TypedJwe_* wrapper."""
    return name.startswith("TypedJws_") or name.startswith("TypedJwe_")


def get_wrapper_title(name: str) -> str:
    """Convert TypedJws_DeviceHsmState -> TypedJws<DeviceHsmState>."""
    if name.startswith("TypedJws_"):
        return f"TypedJws<{name[9:]}>"
    if name.startswith("TypedJwe_"):
        return f"TypedJwe<{name[9:]}>"
    return name


def type_name_to_filename(name: str) -> str:
    """Convert type name to kebab-case filename.
    
    Examples:
        HsmWorkerRequestDto -> hsm-worker-request-dto.md
        TypedJws_DeviceHsmState -> typed-jws-wrapper.md
        Status -> status.md
    """
    if is_wrapper_type(name):
        # All TypedJws_* types go to typed-jws-wrapper.md
        if name.startswith("TypedJws_"):
            return "typed-jws-wrapper.md"
        # All TypedJwe_* types go to typed-jwe-wrapper.md
        return "typed-jwe-wrapper.md"
    
    # Convert PascalCase to kebab-case
    s1 = re.sub('(.)([A-Z][a-z]+)', r'\1-\2', name)
    return re.sub('([a-z0-9])([A-Z])', r'\1-\2', s1).lower() + '.md'


def resolve_type_name(schema: Any, schemas: Dict[str, Any]) -> str:
    """
    Resolve a schema to a markdown-formatted type display string with links.
    
    Returns:
        Markdown string with type name and links (e.g., "[TypedJws](typed-jws-wrapper.md)\\<[DeviceHsmState](device-hsm-state.md)\\>")
    """
    if isinstance(schema, dict):
        # Handle $ref
        if "$ref" in schema:
            ref_name = schema["$ref"].split("/")[-1]
            if is_wrapper_type(ref_name):
                # Handle wrapper types: create links to both wrapper and inner type
                # Extract the inner type name from TypedJws_DeviceHsmState or TypedJwe_InnerRequest
                if ref_name.startswith("TypedJws_"):
                    wrapper_name = "TypedJws"
                    inner_type = ref_name[9:]  # Remove "TypedJws_" prefix
                elif ref_name.startswith("TypedJwe_"):
                    wrapper_name = "TypedJwe"
                    inner_type = ref_name[9:]  # Remove "TypedJwe_" prefix
                else:
                    wrapper_name = ref_name
                    inner_type = None
                
                wrapper_link = type_name_to_filename(ref_name)
                
                if inner_type:
                    # Create link to inner type
                    inner_link = type_name_to_filename(inner_type)
                    return f"[{wrapper_name}]({wrapper_link})\\<[{inner_type}]({inner_link})\\>"
                else:
                    return f"[{wrapper_name}]({wrapper_link})"
            
            # Regular type reference
            return f"[{ref_name}]({type_name_to_filename(ref_name)})"
        
        # Handle type field
        schema_type = schema.get("type")
        
        # Handle type as array (e.g., ["string", "null"] for optional types)
        if isinstance(schema_type, list):
            # Filter out "null" to get the actual type(s)
            non_null_types = [t for t in schema_type if t != "null"]
            if len(non_null_types) == 1:
                # Single non-null type, use it
                schema_type = non_null_types[0]
            elif len(non_null_types) > 1:
                # Multiple non-null types, join them
                return " | ".join(non_null_types)
            else:
                # Only null type
                return "null"
        
        if schema_type == "array":
            item_name = resolve_type_name(schema.get("items", {}), schemas)
            return f"array of {item_name}"
        
        if schema_type == "object":
            # Check if it has additionalProperties (map type)
            additional = schema.get("additionalProperties")
            if additional:
                val_name = resolve_type_name(additional, schemas)
                return f"map of string → {val_name}"
            return "object"
        
        if schema_type == "string":
            # Check for format or enum
            fmt = schema.get("format")
            if fmt:
                return f"string ({fmt})"
            if "enum" in schema:
                return "string (enum)"
            return "string"
        
        if schema_type == "integer":
            fmt = schema.get("format", "int32")
            return f"integer ({fmt})"
        
        if schema_type == "number":
            return "number"
        
        if schema_type == "boolean":
            return "boolean"
        
        # Check for oneOf/anyOf/allOf
        if "oneOf" in schema:
            # Special case: Option<T> pattern (oneOf with null and a $ref)
            one_of = schema["oneOf"]
            if len(one_of) == 2:
                # Check if this is an Option<T> pattern: [null, $ref]
                has_null = any(v.get("type") == "null" for v in one_of)
                non_null = [v for v in one_of if v.get("type") != "null"]
                
                if has_null and len(non_null) == 1:
                    # This is an Option<T>, return the non-null type
                    return resolve_type_name(non_null[0], schemas)
            
            # General oneOf handling
            variants = []
            for variant in one_of:
                # Skip null variants in display
                if variant.get("type") == "null":
                    continue
                v_name = resolve_type_name(variant, schemas)
                variants.append(v_name)
            
            if len(variants) == 1:
                return variants[0]
            return " | ".join(variants)
        
        if schema_type:
            return schema_type
    
    return "unknown"


def generate_field_table(schema: Dict[str, Any], schemas: Dict[str, Any]) -> str:
    """Generate markdown table for struct fields."""
    properties = schema.get("properties", {})
    required = set(schema.get("required", []))
    
    if not properties:
        return ""
    
    lines = [
        "## Fields",
        "",
        "| Field | Type | Required | Description |",
        "|-------|------|----------|-------------|",
    ]
    
    for field_name, field_schema in properties.items():
        # Get the serialized name (from original schema, might be camelCase)
        display_name = f"`{field_name}`"
        
        # Resolve type - now returns just a string
        type_display = resolve_type_name(field_schema, schemas)
        
        # Required/Optional
        is_required = field_name in required
        required_text = "Yes" if is_required else "No"
        
        # Description - handle oneOf pattern where description is in non-null variant
        description = field_schema.get("description", "")
        if not description and "oneOf" in field_schema:
            # Extract description from non-null variant in oneOf
            for variant in field_schema["oneOf"]:
                if variant.get("type") != "null":
                    variant_desc = variant.get("description", "")
                    if variant_desc:
                        description = variant_desc
                        break
        
        # Escape pipe characters in description
        description = description.replace("|", "\\|")
        
        lines.append(f"| {display_name} | {type_display} | {required_text} | {description} |")
    
    lines.append("")
    return "\n".join(lines)


def generate_enum_table(schema: Dict[str, Any]) -> str:
    """Generate markdown table for enum variants."""
    enum_values = schema.get("enum", [])
    
    if not enum_values:
        return ""
    
    lines = [
        "## Variants",
        "",
        "| Variant | Description |",
        "|---------|-------------|",
    ]
    
    # Try to extract descriptions from oneOf if available
    one_of = schema.get("oneOf", [])
    variant_descriptions = {}
    
    for variant_schema in one_of:
        if "const" in variant_schema:
            const_val = variant_schema["const"]
            desc = variant_schema.get("description", "")
            variant_descriptions[const_val] = desc
    
    for value in enum_values:
        desc = variant_descriptions.get(value, "")
        desc = desc.replace("|", "\\|")
        lines.append(f"| `{value}` | {desc} |")
    
    lines.append("")
    return "\n".join(lines)


def get_schema_metadata(schema: Dict[str, Any]) -> List[str]:
    """Extract schema metadata (serialization format, constraints, etc.)."""
    metadata = []
    
    # Check for pattern
    if "pattern" in schema:
        metadata.append(f"**Pattern:** `{schema['pattern']}`")
    
    # Check for format
    if "format" in schema:
        metadata.append(f"**Format:** {schema['format']}")
    
    # Check for minLength/maxLength
    if "minLength" in schema or "maxLength" in schema:
        min_len = schema.get("minLength", "")
        max_len = schema.get("maxLength", "")
        if min_len and max_len:
            metadata.append(f"**Length:** {min_len}-{max_len} characters")
        elif min_len:
            metadata.append(f"**Minimum length:** {min_len} characters")
        elif max_len:
            metadata.append(f"**Maximum length:** {max_len} characters")
    
    return metadata


def generate_markdown(name: str, schema: Dict[str, Any], schemas: Dict[str, Any]) -> str:
    """Generate complete markdown document for a type."""
    lines = []
    
    # Title
    if is_wrapper_type(name):
        title = get_wrapper_title(name)
    else:
        title = name
    lines.append(f"# {title}")
    lines.append("")
    
    # Description
    description = schema.get("description", "")
    if description:
        lines.append(description)
        lines.append("")
    
    # Check if it's an enum
    is_enum = "enum" in schema or schema.get("type") == "string" and "oneOf" in schema
    
    if is_enum:
        # Generate enum table
        enum_table = generate_enum_table(schema)
        if enum_table:
            lines.append(enum_table)
    else:
        # Generate field table for structs
        field_table = generate_field_table(schema, schemas)
        if field_table:
            lines.append(field_table)
    
    # Schema metadata
    metadata = get_schema_metadata(schema)
    if metadata:
        lines.append("## Schema Details")
        lines.append("")
        for item in metadata:
            lines.append(f"- {item}")
        lines.append("")
    
    # For wrapper types, add explanation
    if is_wrapper_type(name):
        lines.append("## About JWS Wrappers")
        lines.append("")
        lines.append("The `TypedJws<T>` type is a type-safe wrapper around JWS (JSON Web Signature) ")
        lines.append("compact serialization strings. The generic type parameter `T` indicates ")
        lines.append("what payload type is signed inside, providing compile-time safety to prevent ")
        lines.append("mixing up different JWS types.")
        lines.append("")
        lines.append("At runtime, this is a string containing a JWS in compact serialization format ")
        lines.append("(RFC 7515): `header.payload.signature` where each part is base64url-encoded.")
        lines.append("")
    
    return "\n".join(lines)


def find_referenced_wrappers(schemas: Dict[str, Any], types_to_doc: List[str]) -> Set[str]:
    """Find all TypedJws_* and TypedJwe_* wrappers referenced by documented types."""
    wrappers = set()
    
    def scan_schema(schema: Any):
        """Recursively scan schema for $ref to wrapper types."""
        if isinstance(schema, dict):
            if "$ref" in schema:
                ref_name = schema["$ref"].split("/")[-1]
                if is_wrapper_type(ref_name):
                    wrappers.add(ref_name)
            
            for value in schema.values():
                scan_schema(value)
        elif isinstance(schema, list):
            for item in schema:
                scan_schema(item)
    
    for type_name in types_to_doc:
        if type_name in schemas:
            scan_schema(schemas[type_name])
    
    return wrappers


def main():
    if len(sys.argv) < 3:
        print("Usage: openapi_to_markdown.py <openapi.json> <output_dir>")
        sys.exit(1)
    
    input_file = sys.argv[1]
    output_dir = Path(sys.argv[2])
    
    # Load OpenAPI spec
    openapi = load_openapi(input_file)
    schemas = extract_schemas(openapi)
    
    # Find all wrapper types referenced by our documented types
    wrapper_types = find_referenced_wrappers(schemas, TYPES_TO_DOCUMENT)
    
    # Combine main types and wrappers
    all_types_to_doc = TYPES_TO_DOCUMENT + list(wrapper_types)
    
    # Generate markdown for each type
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # Track which wrapper file we've created (since multiple TypedJws_* go to same file)
    created_wrapper_files = set()
    
    for type_name in all_types_to_doc:
        if type_name not in schemas:
            print(f"Warning: Type '{type_name}' not found in OpenAPI schemas", file=sys.stderr)
            continue
        
        schema = schemas[type_name]
        markdown = generate_markdown(type_name, schema, schemas)
        
        filename = type_name_to_filename(type_name)
        
        # For wrapper types, we create separate files for TypedJws and TypedJwe
        if is_wrapper_type(type_name):
            if filename in created_wrapper_files:
                # Skip - already created this wrapper file
                continue
            created_wrapper_files.add(filename)
            
            # Create wrapper-specific documentation
            if filename == "typed-jws-wrapper.md":
                markdown = f"""# TypedJws\\<T\\>

A type-safe wrapper around JWS (JSON Web Signature) compact serialization strings.

## Description

The `TypedJws<T>` type wraps a JWS compact serialization string with a phantom type parameter `T` 
that indicates what payload type is signed inside the JWS.

This provides compile-time type safety, preventing you from accidentally passing a 
`TypedJws<DeviceHsmState>` where a `TypedJws<OuterResponse>` is expected, even though both are 
represented as strings at runtime.

## Structure

At runtime, a `TypedJws<T>` is a string in JWS compact serialization format (RFC 7515):

```
header.payload.signature
```

Where:
- `header`: Base64url-encoded JSON header (contains algorithm, key ID, etc.)
- `payload`: Base64url-encoded JSON payload of type `T`
- `signature`: Base64url-encoded cryptographic signature

## Type Safety

The generic type parameter `T` is a phantom type - it exists only at compile time and is 
erased at runtime. This allows the Rust type system to track what's inside each JWS without 
any runtime overhead.

### Example Usage in This API

- `TypedJws<DeviceHsmState>` - Contains signed device state
- `TypedJws<OuterRequest>` - Contains signed outer request envelope
- `TypedJws<OuterResponse>` - Contains signed outer response envelope

## Serialization

When serialized to JSON, a `TypedJws<T>` is transparent - it appears as a plain string:

```json
"eyJhbGciOiJFUzI1NiJ9.eyJ2ZXJzaW9uIjoxfQ.signature..."
```

This transparency ensures compatibility with standard JWS libraries and tools.
"""
            elif filename == "typed-jwe-wrapper.md":
                markdown = f"""# TypedJwe\\<T\\>

A type-safe wrapper around JWE (JSON Web Encryption) compact serialization strings.

## Description

The `TypedJwe<T>` type wraps a JWE compact serialization string with a phantom type parameter `T` 
that indicates what payload type is encrypted inside the JWE.

This provides compile-time type safety, preventing you from accidentally passing a 
`TypedJwe<InnerRequest>` where a `TypedJwe<InnerResponse>` is expected, even though both are 
represented as strings at runtime.

## Structure

At runtime, a `TypedJwe<T>` is a string in JWE compact serialization format (RFC 7516):

```
header.encrypted_key.iv.ciphertext.tag
```

Where:
- `header`: Base64url-encoded JSON header (contains algorithm, encryption method, etc.)
- `encrypted_key`: Base64url-encoded encrypted content encryption key
- `iv`: Base64url-encoded initialization vector
- `ciphertext`: Base64url-encoded encrypted payload of type `T`
- `tag`: Base64url-encoded authentication tag

## Type Safety

The generic type parameter `T` is a phantom type - it exists only at compile time and is 
erased at runtime. This allows the Rust type system to track what's inside each JWE without 
any runtime overhead.

### Example Usage in This API

- `TypedJwe<InnerRequest>` - Contains encrypted inner request payload
- `TypedJwe<InnerResponse>` - Contains encrypted inner response payload

## Serialization

When serialized to JSON, a `TypedJwe<T>` is transparent - it appears as a plain string:

```json
"eyJhbGc...encrypted content...tag"
```

This transparency ensures compatibility with standard JWE libraries and tools.
"""
            else:
                # Fallback for other wrappers
                markdown = generate_markdown(type_name, schema, schemas)
        
        output_path = output_dir / filename
        with open(output_path, 'w') as f:
            f.write(markdown)
        
        print(f"Generated {output_path}")
    
    print(f"\nSuccessfully generated {len(all_types_to_doc)} type documentation files")


if __name__ == "__main__":
    main()
