# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""Misuse the stub must refuse (S3, D43).

The companion to `accepts.py`, and the half that matters more: a stub
that has quietly degraded to `Any` — a lost `py.typed`, a parameter
widened to `object`, a class the checker stopped resolving — still lets
`accepts.py` pass. It stops refusing what is wrong.

Every line below is expected to fail, and says with which mypy error
code in an `# expect:` comment. The test asserts each one produces
exactly that code, and that no line produces an error nobody expected —
so this file cannot silently start passing, and cannot start failing for
a reason other than the one it was written for.
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
