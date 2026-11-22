"""Tiny DSL for writing registry/codelists/*.yaml."""
import json, re

CODE_RE = re.compile(r"^[A-Za-z0-9_.+<>-]{1,64}$")
DEFS = {}


def c(code, en, p=None, s=None, si=None, ta=None):
    return {"code": str(code), "en": en, "parent": p, "sdmx": s, "si": si, "ta": ta, "total": False}


def T(en="Total", si=None, ta=None):
    return {"code": "total", "en": en, "parent": None, "sdmx": "_T", "si": si, "ta": ta, "total": True}


def cl(id, title, codes, source, notes=(), sdmx_ref=None, status="complete", total_parent=True, review=None):
    """status: complete | seed (members must be completed from the source extract).
    total_parent: give every top-level code parent=total when a total exists.
    review: extra native-language review note (default: si/ta pending)."""
    assert id not in DEFS, id
    has_total = any(x["total"] for x in codes)
    seen = set()
    for x in codes:
        assert CODE_RE.match(x["code"]), (id, x["code"])
        assert x["code"] not in seen, (id, x["code"])
        seen.add(x["code"])
        if has_total and total_parent and not x["total"] and x["parent"] is None:
            x["parent"] = "total"
    for x in codes:
        assert x["parent"] is None or x["parent"] in seen, (id, x["code"], x["parent"])
    DEFS[id] = dict(id=id, title=title, codes=codes, source=list(source) if not isinstance(source, str) else [source],
                    notes=list(notes), sdmx_ref=sdmx_ref, status=status, has_total=has_total, review=review)


def q(v):
    s = str(v)
    if re.match(r"^[A-Za-z][A-Za-z0-9 _.()/-]*$", s) and not re.search(r"(: | #)", s) and s.lower() not in (
            "yes", "no", "true", "false", "on", "off", "null", "y", "n"):
        return s
    return json.dumps(s, ensure_ascii=False)


def qcode(v):
    s = str(v)
    if re.match(r"^[A-Za-z_][A-Za-z0-9_]*$", s) and s.lower() not in ("yes", "no", "true", "false", "on", "off", "null", "y", "n"):
        return s
    return json.dumps(s)


def emit(d, used_by):
    out = []
    out.append(f"# {d['title']}")
    for s in d["source"]:
        out.append(f"# Source/authority: {s}")
    for n in d["notes"]:
        out.append(f"# Note: {n}")
    if d["status"] == "seed":
        out.append("# Status: SEED - members below are the confidently known ones; complete the list from the")
        out.append("#   source extract when the connector is built (every observed code must be listed).")
    else:
        out.append("# Status: complete for the classification as cited.")
    if not d["has_total"]:
        out.append("# No total member: the dimension is a non-additive selector; every observation carries an explicit code.")
    have_si = any(x["si"] for x in d["codes"])
    have_ta = any(x["ta"] for x in d["codes"])
    if d["review"]:
        out.append(f"# Labels: {d['review']}")
    elif have_si or have_ta:
        out.append("# Labels: si/ta given only where certain; remaining si/ta pending native-speaker review.")
    else:
        out.append("# Labels: English only; si/ta pending native-speaker review (not machine-translated).")
    out.append("# Order: codes are listed in display order.")
    if used_by:
        out.append("# Used by: " + ", ".join(used_by))
    out.append(f"id: {d['id']}")
    out.append(f"title: {q(d['title'])}")
    if d["sdmx_ref"]:
        out.append(f"sdmx_ref: {q(d['sdmx_ref'])}")
    out.append("codes:")
    for x in d["codes"]:
        if x["si"] or x["ta"]:
            lab = "{ en: " + q(x["en"])
            if x["si"]:
                lab += ", si: " + json.dumps(x["si"], ensure_ascii=False)
            if x["ta"]:
                lab += ", ta: " + json.dumps(x["ta"], ensure_ascii=False)
            lab += " }"
        else:
            lab = q(x["en"])
        parts = [f"code: {qcode(x['code'])}", f"label: {lab}"]
        if x["parent"]:
            parts.append(f"parent: {qcode(x['parent'])}")
        if x["total"]:
            parts.append("total: true")
        if x["sdmx"]:
            parts.append(f"sdmx: {qcode(x['sdmx'])}")
        out.append("  - { " + ", ".join(parts) + " }")
    return "\n".join(out) + "\n"
