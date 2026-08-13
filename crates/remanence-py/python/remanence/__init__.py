# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""Self-contained disk image analysis library.

This package is the compiled extension and nothing more: the shim below
re-exports it, and `__init__.pyi` beside it is the typed surface. It is
written out rather than generated because a mixed maturin layout does
not generate one, and the stub has to live in a package that exists.
"""

from .remanence import *

__doc__ = remanence.__doc__
if hasattr(remanence, "__all__"):
    __all__ = remanence.__all__
