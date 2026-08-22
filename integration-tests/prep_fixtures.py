#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = ["reliquary==0.1.0a3"]
# ///
# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""Prepares test fixtures for remanence-lib unit tests.

The HDOS 1.0 distribution zip downloads sha256-pinned straight into
integration-tests/fixtures/ — a multi-image zip, test material in its
own right. The one disk image the tests read extracts beside it,
joined by a generated single-image zip fixture. (Downloads that are
not fixtures at all land one directory over, in
integration-tests/downloads/ — an archive nothing reads directly,
held only so a fixture can be repackaged out of it.)

The FreeDOS rig artifact is built by driving reliquary through its
Python API. The LiveCD downloads through the blueprint's own media
spec; the prep script pins reliquary's media cache to
test-fixture-prep/test-rigs/cache/media, so the download survives
`cargo clean` and machine rebuilds.

This file carries reliquary's pin as inline script metadata (PEP
723) rather than sitting in a uv project of its own, so `uv run`
provisions the environment straight from the block above — nothing
to sync or activate first, and no pyproject.toml or lock file beside
it:

    uv run integration-tests/prep_fixtures.py
"""

import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
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
        "reliquary is not importable — run this script through uv, "
        "which reads its inline script metadata for the pin:\n"
        "  uv run integration-tests/prep_fixtures.py"
    )

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
FIXTURES_DIR = SCRIPT_DIR / "fixtures"
DOWNLOADS_DIR = SCRIPT_DIR / "downloads"
RIG_DIR = REPO_ROOT / "test-fixture-prep" / "test-rigs"

# See also: https://archive.org/details/flux_capacity
#           https://archive.org/details/20250402_20250402_0519
#           https://archive.org/details/20240119_20240119_0659
#           https://archive.org/details/digitoxin_20260110_213112
#           https://archive.org/details/Populous2_Electronic_Arts_AtariST_DiskImage
#           https://archive.org/details/kings-quest-iii-kixxr-xl-c-sierra-on-line-inc.
#           https://archive.org/details/pcdos_7_de

HDOS_URL = "https://sebhc.github.io/sebhc/software/HDOS/HDOS_1-0.zip"
# The SEBHC archive publishes no hash; this pin records the archive as
# first fetched (2026-07-31), so a changed upstream is caught, not
# silently adopted.
HDOS_ZIP_SHA256 = \
    "84c3217491ed9330eec3134c4809134e68c6e6048a857587750a6571aa81ffe2"
HDOS_IMAGE_NAME = "HDOS_1-0_Issue_#50-00-00_890-1.h8d"
HDOS_ZIP_NAME = "HDOS_1-0_Issue_#50-00-00_890-1.zip"

CPM_2202_URL = \
    "https://sebhc.github.io/sebhc/software/CPM/CPM_2_2_02_Distribution.zip"
# As with the HDOS archive above, SEBHC publishes no hash; this pin
# records the archive as first fetched (2026-08-16), so a changed
# upstream is caught, not silently adopted.
CPM_2202_ZIP_SHA256 = \
    "0b8931989f6f6486fd16c9f90c8235317cac0df7df0f84385dd3da74303f2547"
CPM_2202_ZIP_NAME = "CPM_2_2_02_Distribution.zip"
# The distribution is two disks; the first carries the system and the
# transient commands, which is what the CP/M reader's tests read.
CPM_2202_IMAGE_NAME = "CPM_2_2_02_distribution_1.h8d"

CPM_2203_URL = \
    "https://sebhc.github.io/sebhc/software/CPM/CPM_2_2_03_Distribution.zip"
# First fetched 2026-08-16, pinned on the same terms as its sibling.
CPM_2203_ZIP_SHA256 = \
    "aaa1ed37d7868f361f6cd680b867fe208e9dd4bf1f9f74c863a621f1a0a0a0fc"
CPM_2203_ZIP_NAME = "CPM_2_2_03_Distribution.zip"
# Three disks, and the archive's members carry no release in their
# names. Two are taken: the first for its system and transients, and the
# third because its BIOS.ASM spans four extents — which is the directory
# grammar's join tested against a real artifact rather than a built one.
CPM_2203_IMAGE_NAMES = (
    "CPM_2_2_distribution_1.h8d",
    "CPM_2_2_distribution_3.h8d",
)

# The same release imaged from soft-sectored media, as ImageDisk rather
# than a raw dump. It is the artifact D60 was decided against: its
# interleave is in the sector numbering the format states, where the
# hard-sectored dumps leave it to the drive's BIOS.
CPM_2203_SOFT_URL = (
    "https://heathkit.garlanger.com/software/library/Heath/CPM/"
    "CPM_2_2_03/CPM_2_2_03.zip"
)
# First fetched 2026-08-16; the archive publishes no hash of its own.
CPM_2203_SOFT_ZIP_SHA256 = \
    "25d9434859e7c8c107d4404207f83d8da1f2e55e0cc72f2d3d6a54d82ff854e9"
CPM_2203_SOFT_ZIP_NAME = "CPM_2_2_03_soft.zip"
CPM_2203_SOFT_IMAGE_NAME = "cpm_2_2_03_Disk_1.imd"

# PC-DOS 1.00, the release that wrote no BPB at all. The archive
# carries the same disk as a raw image and as ImageDisk, which is why
# it serves two readers: the media-descriptor path that reads a 1.x
# volume, and the ImageDisk adapter, for which it is the second and
# first non-Heath artifact.
# The download *page*, not a mirror: WinWorld serves each file from
# several mirrors, each behind its own `/from/<mirror>` link on this
# page, and any one of them can 404 for a while. The page is scraped
# for the current set and each is tried in turn (`_winworld_mirrors`).
PCDOS_100_PAGE_URL = \
    "https://winworldpc.com/download/5ee2809d-03c2-b6c2-adc3-9711c3a5c28f"
# WinWorld publishes no hash; first fetched 2026-08-16.
PCDOS_100_ARCHIVE_SHA256 = \
    "7187788b8c1e4f441428e81785134d433a014f7bd1252fd9f1417a9a55dfe65d"
PCDOS_100_ARCHIVE_NAME = "IBM PC-DOS 1.00 (5.25-160k).7z"
PCDOS_100_MEMBERS = (
    ("IBM PC-DOS 1.00 (5.25-160k)/Images/Raw/Disk01.img", "pcdos-100-disk01.img"),
    (
        "IBM PC-DOS 1.00 (5.25-160k)/Images/ImageDisk/DISK01.IMD",
        "pcdos-100-disk01.imd",
    ),
)

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

# IBM PC DOS 7 (German), disk 1 of 5: a KryoFlux capture converted to an
# HxC MFM container by HxC's own tool, which is what makes it the
# artifact the `.mfm` reader is proved against — a real writer rather
# than a synthetic one that shares the reader's assumptions. A plain
# 1.44 MB FAT12 system disk, every sector of it readable, so the flux
# ladder is provable end to end through the FAT volume. Fixture as
# downloaded: nothing to extract.
PCDOS_7_MFM_URL = (
    "https://archive.org/download/pcdos_7_de/pcdos_7_de_disk1.mfm"
)
# Archive.org publishes an MD5 (134d2234f2457fd7674106e98a16ac54), which
# the file as first fetched (2026-08-22) matches; this is its SHA-256.
PCDOS_7_MFM_SHA256 =     "346139f444e0e321bc6bbc2a9993ff74947925fc1fc429bf4c2e3110330c5678"
PCDOS_7_MFM_NAME = "pcdos-7-de-disk1.mfm"

RIG_BLUEPRINT = "remanence-parttest"
# The blueprint's hard-drive slot, whose image is the artifact.
RIG_HDD = "hdd0"
FREEDOS_QCOW2_NAME = "freedos-parttest.qcow2"


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _download_archive(target: Path, url: str, sha256: str) -> None:
    """Download ``url`` to ``target`` unless a verified copy is present."""
    _download_archive_from(target, [url], sha256)


def _download_archive_from(target: Path, urls: list[str], sha256: str) -> None:
    """Download ``target`` from the first of ``urls`` that serves it.

    A URL that fails with an HTTP or network error is skipped and the
    next tried; only when every one fails is the last error raised.
    A URL that serves the wrong bytes is not retried elsewhere — that
    is a changed artifact, not a dead mirror, and the pin decides.
    """
    if target.exists():
        if _sha256(target) == sha256:
            print(f"{target.name} already downloaded")
            return
        print(f"{target.name} fails verification; refetching...")
        target.unlink()

    def report(blocks: int, block_size: int, total: int) -> None:
        done = blocks * block_size
        if total >> 20 and blocks % 512 == 0:
            print(f"\r  {done >> 20} / {total >> 20} MiB", end="", flush=True)

    partial = target.with_suffix(target.suffix + ".part")
    for index, url in enumerate(urls):
        print(f"Downloading {url}...")
        try:
            urllib.request.urlretrieve(url, partial, reporthook=report)
        except (urllib.error.URLError, OSError) as error:
            print()
            partial.unlink(missing_ok=True)
            if index + 1 == len(urls):
                raise
            print(f"  {error}; trying the next mirror")
            continue
        print()
        break
    actual = _sha256(partial)
    if actual != sha256:
        partial.unlink()
        sys.exit(
            f"{url} downloaded with SHA-256 {actual}, but "
            f"{sha256} is pinned; refusing the archive."
        )
    partial.replace(target)


_WINWORLD_MIRROR_LINK = re.compile(
    r'href="(/download/[0-9a-f-]+/from/[0-9a-f-]+)"')


def _winworld_mirrors(page_url: str) -> list[str]:
    """The mirror URLs a WinWorld download page currently offers.

    WinWorld lists its mirrors as ``/download/<file>/from/<mirror>``
    links; the page is the only place the current set is published,
    and the ones on it are returned in the page's own order. An empty
    list is an error: a page with no mirror on it is a changed site,
    not a file with nowhere to come from.
    """
    print(f"Reading mirrors from {page_url}...")
    with urllib.request.urlopen(page_url) as response:
        page = response.read().decode("utf-8", errors="replace")
    mirrors: list[str] = []
    for path in _WINWORLD_MIRROR_LINK.findall(page):
        url = urllib.parse.urljoin(page_url, path)
        if url not in mirrors:
            mirrors.append(url)
    if not mirrors:
        sys.exit(f"{page_url} lists no download mirror; the page has changed.")
    return mirrors


def download_archives() -> None:
    print("==> Downloading external archives...")
    FIXTURES_DIR.mkdir(parents=True, exist_ok=True)
    DOWNLOADS_DIR.mkdir(parents=True, exist_ok=True)
    _download_archive(FIXTURES_DIR / "HDOS_1-0.zip", HDOS_URL,
                      HDOS_ZIP_SHA256)
    _download_archive(FIXTURES_DIR / CPM_2202_ZIP_NAME, CPM_2202_URL,
                      CPM_2202_ZIP_SHA256)
    _download_archive(FIXTURES_DIR / CPM_2203_ZIP_NAME, CPM_2203_URL,
                      CPM_2203_ZIP_SHA256)
    _download_archive(FIXTURES_DIR / CPM_2203_SOFT_ZIP_NAME,
                      CPM_2203_SOFT_URL, CPM_2203_SOFT_ZIP_SHA256)
    _download_archive_from(DOWNLOADS_DIR / PCDOS_100_ARCHIVE_NAME,
                           _winworld_mirrors(PCDOS_100_PAGE_URL),
                           PCDOS_100_ARCHIVE_SHA256)
    _download_archive(DOWNLOADS_DIR / PINBALL_ARCHIVE_NAME, PINBALL_URL,
                      PINBALL_ARCHIVE_SHA256)
    # A single NIB-compressed disk image, fixture as downloaded — no
    # member to extract and nothing to repackage, so it lands straight
    # in the fixtures directory.
    _download_archive(FIXTURES_DIR / CONTRA_NBZ_NAME, CONTRA_URL,
                      CONTRA_NBZ_SHA256)
    _download_archive(FIXTURES_DIR / BEACH_HEAD_ZIP_NAME, BEACH_HEAD_URL,
                      BEACH_HEAD_ZIP_SHA256)
    _download_archive(FIXTURES_DIR / PCDOS_7_MFM_NAME, PCDOS_7_MFM_URL,
                      PCDOS_7_MFM_SHA256)


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


def prepare_cpm_fixtures() -> None:
    """Extract the CP/M system disks beside their archives.

    Heath's CP/M records nothing on the disk about how to read it — the
    disk parameter block lived in the BIOS — so these artifacts are what
    the declared layout is checked against, and they are also where that
    layout was derived from in the first place.

    Two releases, because one could not answer the question the second
    did: 2.2.02 and 2.2.03 turn out to write the same layout on the same
    drive, which is why the enrolled block is named for the medium and
    not for either release.
    """
    print("==> Preparing CP/M fixture images...")
    FIXTURES_DIR.mkdir(parents=True, exist_ok=True)

    wanted = [(CPM_2202_ZIP_NAME, CPM_2202_IMAGE_NAME)]
    wanted += [(CPM_2203_ZIP_NAME, name) for name in CPM_2203_IMAGE_NAMES]
    wanted += [(CPM_2203_SOFT_ZIP_NAME, CPM_2203_SOFT_IMAGE_NAME)]
    for archive, member in wanted:
        target = FIXTURES_DIR / member
        if target.exists():
            print(f"{member} already present")
            continue
        print(f"Extracting {member} into fixtures directory...")
        with zipfile.ZipFile(FIXTURES_DIR / archive, "r") as zip_ref:
            zip_ref.extract(member, FIXTURES_DIR)


def prepare_pcdos_fixtures() -> None:
    """Extract the PC-DOS 1.00 disk, as a raw image and as ImageDisk.

    The same disk in two containers, which is why both are taken: the
    raw one is what the media-descriptor path reads, and the ImageDisk
    one is the ImageDisk adapter's second artifact and its first from
    outside the Heath world.
    """
    print("==> Preparing PC-DOS 1.00 fixture images...")
    FIXTURES_DIR.mkdir(parents=True, exist_ok=True)

    archive = DOWNLOADS_DIR / PCDOS_100_ARCHIVE_NAME
    for member, name in PCDOS_100_MEMBERS:
        target = FIXTURES_DIR / name
        if target.exists():
            print(f"{name} already present")
            continue
        print(f"Extracting {name} into fixtures directory...")
        with tempfile.TemporaryDirectory() as staging:
            subprocess.run(
                ["7z", "e", "-y", f"-o{staging}", str(archive), member],
                check=True,
                stdout=subprocess.DEVNULL,
            )
            extracted = Path(staging) / Path(member).name
            shutil.copyfile(extracted, target)


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


def retire_leftover_machines(session) -> None:
    """Destroy whatever machine an earlier build left behind.

    `run_script` reuses an existing machine of the blueprint as it
    stands, hdd0 included — and the install script partitions a disk
    it takes to be blank. Driven over a disk an interrupted build had
    already partitioned, its `fdisk` steps land wherever the old table
    leaves room and the formats go to whichever letters that produces,
    and what is harvested is a disk no run ever described: extra
    logicals, some never formatted. A leftover stays for inspection
    until the next build, no longer.
    """
    for state in session.list_machines(blueprint=RIG_BLUEPRINT):
        machine_id = state["id"]
        print(f"Destroying leftover machine {machine_id} (a fresh build "
              "starts from a blank disk)...")
        try:
            if state.get("phase") == "running":
                # A guest that powered itself off still reads as
                # running until a stop reconciles it.
                session.stop_machine(machine_id)
            session.destroy_machine(machine_id)
        except reliquary.ReliquaryError as error:
            sys.exit(
                f"could not destroy leftover machine {machine_id}: {error}\n"
                "(see test-fixture-prep/test-rigs/README.md for the rlq "
                "commands that reset a machine by hand)"
            )


def build_rig_machine(session) -> str:
    """Drive the rig's install script on a fresh machine; return its id.

    Returning from inside the `try` is what keeps the caller's name
    unconditionally bound: the only other way out is `sys.exit`, which
    never reaches a caller at all.
    """
    retire_leftover_machines(session)
    try:
        # run_script creates the machine from the blueprint (none exists
        # now), fetches the pinned LiveCD through the blueprint's media
        # spec, and drives the install script; failures raise by error
        # class.
        session.run_script("install", blueprint=RIG_BLUEPRINT)
        return session.resolve_machine(blueprint=RIG_BLUEPRINT)
    except reliquary.ReliquaryError as error:
        sys.exit(
            f"reliquary install run failed: {error}\n"
            "(QEMU missing? A LiveCD whose prompts have moved under the\n"
            "install script? See test-fixture-prep/test-rigs/README.md — the\n"
            "script records which screen texts it depends on.)"
        )


def prepare_freedos_fixture(session) -> None:
    print("==> Preparing FreeDOS qcow2 rig artifact...")
    target = FIXTURES_DIR / FREEDOS_QCOW2_NAME
    if target.exists():
        print(f"FreeDOS fixture already present at {target}")
        return

    print(f"Building {target.name} with reliquary...")
    machine_id = build_rig_machine(session)

    # Harvest the machine's hdd0 image from where the machine's own
    # record says it was materialized: reliquary lays its per-machine
    # images out as it sees fit (the directory moved between releases),
    # and machine.json carries each drive's realized path.
    state = session.load_machine_state(machine_id)
    realized = state.get("drives", {}).get(RIG_HDD, {}).get("path")
    image = Path(realized) if realized else None
    if image is None or not image.is_file():
        sys.exit(
            f"machine {machine_id} records no image for drive {RIG_HDD} "
            f"(path: {realized!r}) — if reliquary materialized it "
            "elsewhere or as another format, the rig blueprint needs "
            "adjusting"
        )
    shutil.copy(image, target)
    print(f"Harvested {image.name} to {target}")
    reclaim_machine(machine_id, session)


def reclaim_machine(machine_id: str, session) -> None:
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
        session.destroy_machine(machine_id)
        print(f"  destroyed machine {machine_id}")
    except reliquary.ReliquaryError as error:
        print(f"  warning: could not destroy {machine_id}: {error}",
              file=sys.stderr)
    try:
        for name in session.prune_media():
            print(f"  pruned media {name}")
    except reliquary.ReliquaryError as error:
        print(f"  warning: could not prune media: {error}", file=sys.stderr)


def clean_rig_media(session) -> None:
    """The blunt final reclaim, once every rig has had its turn.

    Prune keeps whatever the catalog could still attach, which is the
    right answer between machines and the wrong one after the last:
    nothing further is going to run, so the whole cache goes. It is
    all fetchable again from pinned locations.
    """
    try:
        reclaimed = session.clean_media()
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
    prepare_cpm_fixtures()
    prepare_pcdos_fixtures()
    prepare_pinball_fixture()

    # Every rig machine builds, harvests and retires in turn; the
    # media cache is emptied only once the last of them is done.
    session = reliquary.Session(rig_context())
    prepare_freedos_fixture(session)
    clean_rig_media(session)

    print(f"==> Test fixtures ready in {FIXTURES_DIR}")


if __name__ == "__main__":
    main()
