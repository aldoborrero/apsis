"""Shared constants for pyflows."""

import re

COMMENTARY_PATTERN = re.compile(r"commentary|director|description", re.IGNORECASE)

# Spanish dubs come in two variants that share the ``spa`` tag; streaming rips
# (Disney+, Netflix) tell them apart only by the track title. Checked in order, so
# a title written by a previous pass ("Spanish (Spain) / ...") maps back to itself.
SPANISH_VARIANTS = (
    ("Spain", re.compile(r"spain|españa|espana|european|castil|castellano|es-es", re.IGNORECASE)),
    ("Latin America", re.compile(r"latin|latino|latam|419|mexic", re.IGNORECASE)),
)


def spanish_variant(title: str) -> str | None:
    """Return "Spain" or "Latin America" when a Spanish track's title names it."""
    for name, pattern in SPANISH_VARIANTS:
        if pattern.search(title or ""):
            return name
    return None
