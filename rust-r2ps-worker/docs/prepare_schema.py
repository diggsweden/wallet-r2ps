#!/usr/bin/env python3
"""
Prepare a JSON Schema document (domain-schema.json) for json-schema-for-humans
from the project's OpenAPI specification (openapi.json).

Why not feed openapi.json directly?
  json-schema-for-humans expects a stand-alone JSON Schema document, so we
  flatten components/schemas into $defs and add a root "object" with one
  property per schema that acts as the table of contents.

Special handling for Jws_* and InnerJwe_* wrapper schemas:
  These are typed-string wrappers (Jws<T>, InnerJwe<T>) that carry a
  contentSchema/$ref pointing to their inner type.  Two problems arise when
  json-schema-for-humans encounters them naively:

  1. It follows contentSchema and expands the inner schema inline, creating
     deeply-nested path-based anchor IDs such as
     "HsmWorkerRequest_outerRequestJws_contentSchema_inner_jwe_oneOf_i1".
     Subsequent uses of the same $ref then show "Same definition as
     [terrible-id]", which is unreadable.

  2. Because struct schemas (HsmWorkerRequest, etc.) appear before the
     Jws_/InnerJwe_ schemas alphabetically, those structs are processed first.
     The first occurrence of, say, Jws_DeviceHsmState is therefore recorded as
     HsmWorkerRequest.stateJws, and the canonical top-level entry for
     Jws_DeviceHsmState ends up showing "Same definition as stateJws" — the
     wrong direction.

Fixes applied:
  - Wrapper schemas are listed FIRST in both $defs and properties so that
    json-schema-for-humans records them as the canonical first occurrence.
    With --link-to-reused-ref, struct fields then correctly show
    "Same definition as Jws_DeviceHsmState" (pointing to the definition).
  - contentSchema / contentMediaType are stripped before writing
    domain-schema.json to avoid the traversal issue above.
  - The inner type name in each description is replaced with a Markdown link
    (json-schema-for-humans renders descriptions as Markdown), so readers
    can still navigate to the content schema.
  - A human-readable title (e.g. "Jws<DeviceHsmState>") is added so the
    section header is meaningful rather than the raw key name.
"""

import json
import re
import sys

INPUT  = sys.argv[1] if len(sys.argv) > 1 else "openapi.json"
OUTPUT = sys.argv[2] if len(sys.argv) > 2 else "docs/domain-schema.json"


def is_wrapper(name: str) -> bool:
    return name.startswith("Jws_") or name.startswith("InnerJwe_")


def wrapper_title(name: str) -> str:
    """Return a generic-syntax title, e.g. 'Jws<DeviceHsmState>'."""
    if name.startswith("Jws_"): 
        return f"Jws<{name[4:]}>"
    if name.startswith("InnerJwe_"):
        return f"InnerJwe<{name[9:]}>"
    return name


def enhance_wrapper(schema: dict) -> dict:
    """
    Prepare a wrapper schema for HTML rendering:
      - Add a title such as "Jws<DeviceHsmState>".
      - Replace the bare inner type name in the description with a Markdown
        link so readers can navigate there from the rendered page.
      - Remove contentSchema / contentMediaType; these are correct in
        openapi.json but cause traversal problems in json-schema-for-humans.
    """
    schema = dict(schema)  # shallow copy — don't mutate the original

    content_ref = schema.get("contentSchema", {}).get("$ref", "")
    if content_ref:
        inner_type = content_ref.rsplit("/", 1)[-1]
        desc = schema.get("description", "")
        # Replace the last occurrence of the bare type name (followed by a
        # period) with a Markdown link.
        schema["description"] = re.sub(
            r"\b" + re.escape(inner_type) + r"(?=\.)",
            f"[{inner_type}](#{inner_type})",
            desc,
        )

    schema.pop("contentSchema", None)
    schema.pop("contentMediaType", None)
    return schema


with open(INPUT) as f:
    doc = json.load(f)

raw_schemas = doc["components"]["schemas"]
info = doc.get("info", {})

# Split into wrapper schemas and everything else.
wrappers = {}
structs  = {}
for name, schema in raw_schemas.items():
    if is_wrapper(name):
        enhanced = enhance_wrapper(schema)
        enhanced["title"] = wrapper_title(name)
        wrappers[name] = enhanced
    else:
        structs[name] = schema

# Wrappers first so json-schema-for-humans sees them as the canonical
# definition before encountering them as field references inside structs.
ordered = {**wrappers, **structs}

root = {
    "$schema":     "https://json-schema.org/draft/2020-12/schema",
    "title":       info.get("title", "Domain Model"),
    "description": info.get("description", ""),
    "type":        "object",
    "properties":  {k: {"$ref": f"#/$defs/{k}"} for k in ordered},
    "$defs":       ordered,
}

# Rewrite all $ref paths from OpenAPI convention to JSON Schema $defs.
text = json.dumps(root, indent=2).replace(
    "#/components/schemas/", "#/$defs/"
)

with open(OUTPUT, "w") as f:
    f.write(text)

print(f"Wrote {OUTPUT}")
