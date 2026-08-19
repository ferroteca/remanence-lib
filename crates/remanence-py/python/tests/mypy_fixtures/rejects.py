# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""Code the stub must refuse to type-check.

The companion to `accepts.py`, and the half that matters more: a stub
that has quietly degraded to `Any` — a lost `py.typed`, a parameter
widened to `object`, a class the checker stopped resolving — still lets
`accepts.py` pass. It stops refusing what is wrong.

Every line below is invalid for a different reason, each marked with an
`# expect:` comment naming the mypy error code it should produce. Only
the file's overall pass/fail is actually checked (`mypy` is expected to
exit nonzero); the `# expect:` comments are not verified against mypy's
actual output — they document, for a human editing this file, which
mistake each line is meant to demonstrate.
"""

from __future__ import annotations

import remanence

session = remanence.Session()
handle = open("disk.qcow2", "rb")

# A keyword nobody declared.
session.load_media(handle, "qcow2", devise="mbr-block-hd")  # expect: call-arg

# A required keyword-only argument, omitted.
remanence.discover_media("disk.img")  # expect: call-arg

# An Optional used without narrowing.
medium = session.medium(0)
medium.size  # expect: union-attr

# A name the surface does not have — this one was renamed away (D38).
image = remanence.FluxImage("disk.remanence")
image.locations  # expect: attr-defined

# The qualifier this receiver used to carry, kept (D39, overruled by D59).
image.materialize_c1541_bitstream()  # expect: attr-defined

# A path argument given an int.
image.write_d64(3)  # expect: arg-type

# A count read as a string.
sectors_per: str = remanence.Session().new_media("chs-disk").geometry.state
cylinders: str = remanence.Session().new_media("chs-disk").geometry.cylinders  # expect: assignment

# A frozen property, assigned.
partition = remanence.Session().new_media("chs-disk").partition(0)
if partition is not None:
    partition.ordinal = 4  # expect: misc

# A positional-only parameter passed by name.
space = remanence.Session().new_media("chs-disk").partition(0)
if space is not None:
    space.check_type(type_id="dos-primary")  # expect: call-arg

# The exception's attributes, misread.
try:
    remanence.discover_media("missing.img", writable=True)
except remanence.Error as refusal:
    bad_category: int = refusal.category  # expect: assignment
