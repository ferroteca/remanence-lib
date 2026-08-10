#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""Prepares test fixtures for remanence-lib unit tests.

The HDOS 1.0 distribution zip downloads sha256-pinned straight into
crates/remanence/tests/fixtures/ — a multi-image zip, test material
in its own right. The one disk image the tests read extracts beside
it, joined by a generated single-image zip fixture. (Downloads that
are not fixtures at all would land in test-fixture-prep/downloads/.)

The FreeDOS rig artifact is built by driving reliquary through its
Python API. The LiveCD downloads through the blueprint's own media
spec; the prep script pins reliquary's media cache to
test-fixture-prep/test-rigs/cache/media, so the download survives
`cargo clean` and machine rebuilds.

Reliquary is pinned in the root pyproject.toml's `test-fixture-prep`
dependency group; run the script through uv from the repo root:

    uv run --group test-fixture-prep test-fixture-prep/prep_fixtures.py
"""

import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

try:
    import reliquary
except ImportError:
    # Bound on both paths so the name is unconditionally defined at
    # module scope; exiting from inside the handler would leave every
    # later `reliquary.*` reading as possibly-unbound.
    reliquary = None

if reliquary is None:
    sys.exit(
        "reliquary is not importable — run this script through uv "
        "from the repo root:\n"
        "  uv run --group test-fixture-prep test-fixture-prep/prep_fixtures.py"
    )

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURES_DIR = REPO_ROOT / "crates" / "remanence" / "tests" / "fixtures"
DOWNLOADS_DIR = REPO_ROOT / "test-fixture-prep" / "downloads"
RIG_DIR = REPO_ROOT / "test-fixture-prep" / "test-rigs"

# See also: https://archive.org/details/flux_capacity
#           https://archive.org/details/20250402_20250402_0519
#           https://archive.org/details/20240119_20240119_0659
#           https://archive.org/details/digitoxin_20260110_213112
#           https://archive.org/details/Populous2_Electronic_Arts_AtariST_DiskImage
#           https://archive.org/details/kings-quest-iii-kixxr-xl-c-sierra-on-line-inc.

HDOS_URL = "https://sebhc.github.io/sebhc/software/HDOS/HDOS_1-0.zip"
# The SEBHC archive publishes no hash; this pin records the archive as
# first fetched (2026-07-31), so a changed upstream is caught, not
# silently adopted.
HDOS_ZIP_SHA256 = \
    "84c3217491ed9330eec3134c4809134e68c6e6048a857587750a6571aa81ffe2"
HDOS_IMAGE_NAME = "HDOS_1-0_Issue_#50-00-00_890-1.h8d"
HDOS_ZIP_NAME = "HDOS_1-0_Issue_#50-00-00_890-1.zip"

PINBALL_URL = (
    "https://archive.org/download/BillBudgePinballConstructionSetCommodore64.7z/"
    "Disk%20Dump/Bill%20Budge%20Pinball%20Construction%20Set%5BCommodore%20"
    "64%5D.7z"
)
PINBALL_ARCHIVE_NAME = "Bill Budge Pinball Construction Set [Commodore 64].7z"
# Archive.org publishes no hash; this pin records the archive as first fetched
# (2026-08-02), so a changed upstream is caught, not silently adopted.
PINBALL_ARCHIVE_SHA256 = \
    "3cf001b21f76d4c932bbd1a385f13f551fdb01735ce8d12a26c69f3ab9367889"
PINBALL_DISK_ONE_PREFIX = "Bill Budge Pinball Construction Set[Commodore 64](1of2)"
PINBALL_DISK_ONE_NAME = \
    "Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z"
# 84 drive-step positions, each captured by both heads.
PINBALL_DISK_ONE_STEP_COUNT = 84
PINBALL_DISK_ONE_MEMBER_COUNT = PINBALL_DISK_ONE_STEP_COUNT * 2

CONTRA_URL = (
    "https://archive.org/download/"
    "C64_Preservation_Project_10th_Anniversary_Collection/"
    "C64_Preservation_Project_10th_Anniversary_Collection.zip/"
    "c64pp%2Fvmax%2Fcontra%5Bkonami_1988%5D%28vmax3%29%28%21%29.nbz"
)
# Served by extracting one member out of the collection zip on the fly,
# so the pin covers the member, not the collection. Archive.org
# publishes no hash; this records the file as first fetched
# (2026-08-03), so a changed upstream is caught, not silently adopted.
CONTRA_NBZ_SHA256 = \
    "1d8ae0328e2ae66afcd8792b7873fcfbb66d0ce3d7b71e26923cc3dce841e136"
CONTRA_NBZ_NAME = "contra[konami_1988](vmax3)(!).nbz"

BEACH_HEAD_URL = (
    "https://archive.org/download/"
    "C64_Preservation_Project_10th_Anniversary_Collection/"
    "C64_Preservation_Project_10th_Anniversary_Collection_G64.zip/"
    "c64pp-g64-zip%2Fb%2Fbeach-head%5Baccess_1983%5D%28v5%29%28%21%29.zip"
)
# As with the nbz above: served by extracting one member out of the
# collection zip, so the pin covers the member. First fetched
# 2026-08-03. The member is itself a zip, holding one deflated G64.
BEACH_HEAD_ZIP_SHA256 = \
    "f8b532e6d43e082fa76f840ccf0a080d198504b7751381d642f63263f9f680d7"
BEACH_HEAD_ZIP_NAME = "beach-head[access_1983](v5)(!).zip"

RIG_BLUEPRINT = "remanence-parttest"
FREEDOS_QCOW2_NAME = "freedos-parttest.qcow2"


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _download_archive(target: Path, url: str, sha256: str) -> None:
    """Download ``url`` to ``target`` unless a verified copy is present."""
    if target.exists():
        if _sha256(target) == sha256:
            print(f"{target.name} already downloaded")
            return
        print(f"{target.name} fails verification; refetching...")
        target.unlink()

    print(f"Downloading {url}...")

    def report(blocks: int, block_size: int, total: int) -> None:
        done = blocks * block_size
        if total >> 20 and blocks % 512 == 0:
            print(f"\r  {done >> 20} / {total >> 20} MiB", end="", flush=True)

    partial = target.with_suffix(target.suffix + ".part")
    urllib.request.urlretrieve(url, partial, reporthook=report)
    print()
    actual = _sha256(partial)
    if actual != sha256:
        partial.unlink()
        sys.exit(
            f"{url} downloaded with SHA-256 {actual}, but "
            f"{sha256} is pinned; refusing the archive."
        )
    partial.replace(target)


def download_archives() -> None:
    print("==> Downloading external archives...")
    FIXTURES_DIR.mkdir(parents=True, exist_ok=True)
    DOWNLOADS_DIR.mkdir(parents=True, exist_ok=True)
    _download_archive(FIXTURES_DIR / "HDOS_1-0.zip", HDOS_URL,
                      HDOS_ZIP_SHA256)
    _download_archive(DOWNLOADS_DIR / PINBALL_ARCHIVE_NAME, PINBALL_URL,
                      PINBALL_ARCHIVE_SHA256)
    # A single NIB-compressed disk image, fixture as downloaded — no
    # member to extract and nothing to repackage, so it lands straight
    # in the fixtures directory.
    _download_archive(FIXTURES_DIR / CONTRA_NBZ_NAME, CONTRA_URL,
                      CONTRA_NBZ_SHA256)
    _download_archive(FIXTURES_DIR / BEACH_HEAD_ZIP_NAME, BEACH_HEAD_URL,
                      BEACH_HEAD_ZIP_SHA256)


def prepare_hdos_fixtures() -> None:
    print("==> Preparing HDOS fixture images...")
    FIXTURES_DIR.mkdir(parents=True, exist_ok=True)

    hdos_zip = FIXTURES_DIR / "HDOS_1-0.zip"
    hdos_8d_target = FIXTURES_DIR / HDOS_IMAGE_NAME
    hdos_zip_target = FIXTURES_DIR / HDOS_ZIP_NAME

    if not hdos_8d_target.exists():
        # Only the one image the tests read; the archive's other disks
        # stay in the zip until a test wants them.
        print(f"Extracting {HDOS_IMAGE_NAME} into fixtures directory...")
        with zipfile.ZipFile(hdos_zip, "r") as zip_ref:
            zip_ref.extract(HDOS_IMAGE_NAME, FIXTURES_DIR)

    if not hdos_zip_target.exists():
        print(f"Packaging single image ZIP fixture ({hdos_zip_target.name})...")
        with zipfile.ZipFile(hdos_zip_target, "w", zipfile.ZIP_STORED) as zip_out:
            zip_out.write(hdos_8d_target, arcname=hdos_8d_target.name)


def prepare_pinball_fixture() -> None:
    """Package disk one's whole capture -- both heads -- as one 7z fixture.

    This is the artifact a real capture produces: a single-sided 1541 disk
    read in a two-head drive, so the tool captures every step position from
    both heads and the operator archives the lot.  Head 0 carries the disk;
    head 1 is the unrecorded back, and reads as noise.  Telling them apart
    is the library's job, not the fixture's, so both are kept and neither is
    labelled here.

    Members keep the `.0.raw` / `.1.raw` head designator rather than having
    it stripped: the stream itself records no track or side anywhere, so the
    name is the only place a capture's position exists, and it is the
    convention the capture tool writes and the rest of the toolchain reads.
    A fixture renamed out of that convention would admit a grammar no real
    capture has.
    """
    print("==> Preparing Pinball Construction Set KryoFlux fixture...")
    target = FIXTURES_DIR / PINBALL_DISK_ONE_NAME
    if target.exists():
        print("Pinball Construction Set fixture already present")
        return

    seven_zip = shutil.which("7z") or shutil.which("7z.exe")
    if seven_zip is None:
        sys.exit(
            "7-Zip is required to extract and package the Pinball "
            "Construction Set source archive; install 7-Zip and rerun "
            "this script."
        )

    archive = DOWNLOADS_DIR / PINBALL_ARCHIVE_NAME
    with tempfile.TemporaryDirectory() as temp_dir:
        extract_dir = Path(temp_dir)
        result = subprocess.run(
            [seven_zip, "x", "-y", f"-o{extract_dir}", str(archive),
             f"{PINBALL_DISK_ONE_PREFIX}*.0.raw",
             f"{PINBALL_DISK_ONE_PREFIX}*.1.raw"],
            check=False,
        )
        if result.returncode != 0:
            sys.exit(
                "7-Zip could not extract the Pinball Construction Set disk "
                f"one capture (exit code {result.returncode})."
            )

        captures = sorted(
            path for path in extract_dir.iterdir()
            if path.is_file()
            and path.name.startswith(PINBALL_DISK_ONE_PREFIX)
            and (path.name.endswith(".0.raw") or path.name.endswith(".1.raw"))
        )
        if len(captures) != PINBALL_DISK_ONE_MEMBER_COUNT:
            sys.exit(
                "Pinball Construction Set archive yielded "
                f"{len(captures)} disk-one streams; expected "
                f"{PINBALL_DISK_ONE_MEMBER_COUNT} "
                f"({PINBALL_DISK_ONE_STEP_COUNT} step positions, two heads)."
            )

        print(f"Packaging {len(captures)} disk-one streams "
              f"into {target.name}...")
        result = subprocess.run(
            [seven_zip, "a", "-t7z", "-mx=9", str(target), "*.raw"],
            cwd=extract_dir,
            check=False,
        )
        if result.returncode != 0:
            sys.exit(
                f"7-Zip could not package {target.name} "
                f"(exit code {result.returncode})."
            )
        result = subprocess.run([seven_zip, "t", str(target)], check=False)
        if result.returncode != 0:
            sys.exit(
                f"7-Zip could not verify {target.name} "
                f"(exit code {result.returncode})."
            )


def rig_context():
    """The rig home, assigned once; every other directory derives.

    A scoped Context pins the rig's directories without touching
    reliquary's process-global assignments. The rig tree *is* a
    reliquary home, so `blueprints`, `scripts` and `cache` derive from
    it, and `media` and `machines` from that cache — which is why the
    checked-in blueprints/ and scripts/ sit where they do.

    The home itself is not overridable: it holds checked-in files, so
    moving it would only point the run at blueprints and scripts that
    are not there. The **cache** is the part worth relocating, being
    the only part that is bulky and regenerable, and assigning it
    leaves the derivation above it untouched.
    """
    return reliquary.Context(
        home_dir=str(RIG_DIR),
        # None keeps the derived <home>/cache.
        cache_dir=os.environ.get("REMANENCE_TEST_RIG_CACHE_DIR"),
    )


def build_rig_machine(context) -> tuple[str, Path]:
    """Drive the rig's install script; return its machine and directory.

    Returning from inside the `try` is what keeps the caller's names
    unconditionally bound: the only other way out is `sys.exit`, which
    never reaches a caller at all.
    """
    try:
        # run_script creates the machine from the blueprint when none
        # exists, fetches the pinned LiveCD through the blueprint's
        # media spec, and drives the install script; failures raise by
        # error class.
        reliquary.run_script("install", blueprint=RIG_BLUEPRINT, context=context)
        machine_id = reliquary.resolve_machine(
            blueprint=RIG_BLUEPRINT, context=context
        )
        machine_dir = reliquary.get_machine_dir(
            machine=machine_id, context=context
        )
        return machine_id, Path(machine_dir)
    except reliquary.ReliquaryError as error:
        sys.exit(
            f"reliquary install run failed: {error}\n"
            "(QEMU missing? A LiveCD whose prompts have moved under the\n"
            "install script? See test-fixture-prep/test-rigs/README.md — the\n"
            "script records which screen texts it depends on.)"
        )


def prepare_freedos_fixture(context) -> None:
    print("==> Preparing FreeDOS qcow2 rig artifact...")
    target = FIXTURES_DIR / FREEDOS_QCOW2_NAME
    if target.exists():
        print(f"FreeDOS fixture already present at {target}")
        return

    print(f"Building {target.name} with reliquary...")
    machine_id, machine_dir = build_rig_machine(context)

    # Harvest the machine's hdd0 image.
    media_dir = machine_dir / "media"
    image = next(iter(sorted(media_dir.glob("*.qcow2"))), None)
    if image is None:
        sys.exit(
            f"no qcow2 image found under {media_dir} — if reliquary "
            "materialized hdd0 as another format, the rig blueprint needs "
            "adjusting"
        )
    shutil.copy(image, target)
    print(f"Harvested {image.name} to {target}")
    reclaim_machine(machine_id, context)


def reclaim_machine(machine_id: str, context) -> None:
    """Retire one rig machine once its artifact is harvested.

    Destroy, then prune. Pruning is the *safe* reclaim: it drops only
    what falls outside the remaining attachment closure, so media a
    machine still to run would attach survives untouched — which is
    what makes this correct to call per machine rather than once at
    the end. For this rig it reclaims the LiveCD zip, whose extracted
    ISO is already cached and which is one download away if the
    closure ever needs it again.

    Reached only after a successful harvest: a failed run leaves the
    machine, its screenshots and every cached payload exactly where
    they are, because that wreckage is the whole diagnostic.

    Reclaiming never fails the run — the deliverable is already on
    disk, so anything that will not go says so and nothing more.
    """
    print(f"Retiring {machine_id} (its artifact is harvested)...")
    try:
        reliquary.destroy_machine(machine_id, context)
        print(f"  destroyed machine {machine_id}")
    except reliquary.ReliquaryError as error:
        print(f"  warning: could not destroy {machine_id}: {error}",
              file=sys.stderr)
    try:
        for name in reliquary.prune_media(context=context):
            print(f"  pruned media {name}")
    except reliquary.ReliquaryError as error:
        print(f"  warning: could not prune media: {error}", file=sys.stderr)


def clean_rig_media(context) -> None:
    """The blunt final reclaim, once every rig has had its turn.

    Prune keeps whatever the catalog could still attach, which is the
    right answer between machines and the wrong one after the last:
    nothing further is going to run, so the whole cache goes. It is
    all fetchable again from pinned locations.
    """
    try:
        reclaimed = reliquary.clean_media(context=context)
    except reliquary.ReliquaryError as error:
        print(f"warning: could not clean media: {error}", file=sys.stderr)
        return
    if reclaimed:
        print("==> Reclaiming the rig media cache...")
        for name in reclaimed:
            print(f"  cleaned media {name}")


def main() -> None:
    download_archives()
    prepare_hdos_fixtures()
    prepare_pinball_fixture()

    # Every rig machine builds, harvests and retires in turn; the
    # media cache is emptied only once the last of them is done.
    context = rig_context()
    prepare_freedos_fixture(context)
    clean_rig_media(context)

    print(f"==> Test fixtures ready in {FIXTURES_DIR}")


if __name__ == "__main__":
    main()
