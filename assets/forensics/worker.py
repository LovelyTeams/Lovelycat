"""Read a disk image and stream a selected decrypted volume to a staging file."""

import hashlib
import json
import os
import stat
import struct
import sys
import time
import zlib
from contextlib import ExitStack
from pathlib import Path

from dissect.evidence.ewf import EWF, find_files
from dissect.fve.bde import BDE, is_bde_volume
from dissect.fve.luks.luks import LUKS, find_luks_headers
from dissect.hypervisor.disk.vhd import VHD
from dissect.hypervisor.disk.vhdx import VHDX
from dissect.util.stream import RangeStream
from dissect.volume.disk.schemes.mbr import MBR
from dissect.volume.exceptions import DiskError


class Failure(Exception):
    """A message safe to show without exposing a credential."""


EWF_SIGNATURE = b"EVF\x09\x0d\x0a\xff\x00"
CHUNK = 1024 * 1024


def emit(event, data=None):
    print(json.dumps({"event": event, "data": data}), flush=True)


def read_at(stream, offset, size):
    stream.seek(offset)
    return stream.read(size)


def file_info(path):
    info = path.stat()
    if not stat.S_ISREG(info.st_mode):
        raise Failure("请选择普通镜像文件或密钥文件。")
    return info


def file_stamp(paths):
    digest = hashlib.sha256()
    for path in paths:
        info = file_info(path)
        digest.update(str((info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns)).encode("ascii"))
    return digest.hexdigest()


def open_image(path, files):
    length = file_info(path).st_size
    if not length:
        raise Failure("镜像文件为空。")
    source = files.enter_context(path.open("rb"))
    header = read_at(source, 0, 8)
    if header == EWF_SIGNATURE:
        if path.suffix.lower() != ".e01":
            raise Failure("请选择 EWF 镜像的首段 .E01。")
        paths = find_files(path)
        if not paths or paths[0] != path.resolve():
            raise Failure("无法定位 .E01 首段。")
        for number, member in enumerate(paths, 1):
            file_info(member)
            with member.open("rb") as part:
                segment = part.read(13)
            if segment[:8] != EWF_SIGNATURE or int.from_bytes(segment[9:11], "little") != number:
                raise Failure("EWF 镜像的分段缺失或顺序有误。")
        try:
            image = EWF(paths)
            if (
                len(image._segment_offsets) != len(paths) - 1
                or image.segment(len(paths) - 1).sections[-1].type != b"done"
            ):
                raise Failure("EWF 镜像的最后一段缺失或分段损坏。")
            stream = image.open()
            # Read the last partial buffer without asking for sectors past the final segment.
            stream.align = image.volume.sector_size
            return stream, image.size, image.volume.sector_size, "EWF", paths
        except Failure:
            raise
        except Exception:
            raise Failure("无法读取 EWF 镜像，请检查同目录下的所有分段。") from None

    if header == b"vhdxfile":
        # Supply a nameless stream so the parser cannot use paths embedded in a snapshot.
        try:
            source.seek(0)
            image = VHDX(RangeStream(source, 0, length))
        except AttributeError as error:
            if error.name == "parent" and error.obj is None:
                raise Failure("请先将 VHDX 差分链合并为独立镜像，再导入副本。") from None
            raise
        if any(image.header.log_guid):
            raise Failure("VHDX 有待回放的日志。请使用正常关闭后生成的镜像副本。")
        return image, image.size, image.sector_size, "VHDX", [path]

    footer = read_at(source, max(0, length - 512), 512)
    if length >= 512 and footer[:8] == b"conectix":
        disk_type = int.from_bytes(footer[60:64], "big")
        if disk_type not in (2, 3):
            raise Failure("请先将 VHD 差分链合并为独立镜像，再导入副本。")
        image = VHD(source)
        if disk_type == 2 and image.size != length - 512:
            raise Failure("VHD 数据长度与尾部记录不一致。")
        return image, image.size, 512, "VHD", [path]

    if (
        header.startswith((b"EVF2", b"LVF", b"QFI\xfb"))
        or path.suffix.lower() in {".e01", ".ex01", ".vhdx", ".avhdx", ".vhd", ".vmdk", ".qcow2"}
    ):
        raise Failure("无法读取此镜像容器。可导入 RAW、E01 或独立的 VHD / VHDX。")
    return source, length, None, "RAW", [path]


def gpt_partitions(stream, sector_size, size):
    header = read_at(stream, sector_size, sector_size)
    if len(header) < 92:
        raise Failure("GPT 表头不完整。")
    length, checksum = struct.unpack_from("<II", header, 12)
    array_lba, count, width, array_checksum = struct.unpack_from("<QIII", header, 72)
    array_offset = array_lba * sector_size
    array_length = count * width
    if (
        not 92 <= length <= len(header)
        or zlib.crc32(header[:16] + b"\x00" * 4 + header[20:length]) != checksum
        or array_lba < 2
        or not 1 <= count <= 4096
        or width < 128
        or width % 8
        or array_length > 4 * CHUNK
        or array_offset + array_length > size
    ):
        raise Failure("GPT 表头校验失败或分区表超出镜像范围。")
    entries = read_at(stream, array_offset, array_length)
    if len(entries) != array_length or zlib.crc32(entries) != array_checksum:
        raise Failure("GPT 分区表 CRC32 校验失败。")
    for index in range(count):
        entry = entries[index * width : (index + 1) * width]
        if not any(entry[:16]):
            continue
        first, last = struct.unpack_from("<QQ", entry, 32)
        name = entry[56:128].decode("utf-16-le", errors="replace").split("\x00", 1)[0]
        name = "".join(char for char in name if char.isprintable()).strip()
        yield index + 1, first * sector_size, (last - first + 1) * sector_size, name


def encryption(stream):
    _, _, version = find_luks_headers(stream)
    if version in (1, 2):
        return f"luks{version}"
    try:
        if is_bde_volume(stream):
            return "bitlocker"
    except (EOFError, ValueError):
        pass
    return "unknown"


def partitions(stream, size, hint):
    sector_size = hint or 512
    if size <= 0 or sector_size not in (512, 4096):
        raise Failure("镜像的逻辑容量或扇区大小无效。")
    whole = [(None, 0, size, "")]
    if encryption(RangeStream(stream, 0, size)) != "unknown":
        return None, sector_size, whole
    for sector in (hint,) if hint else (512, 4096):
        if read_at(stream, sector, 8) == b"EFI PART":
            return "GPT", sector, list(gpt_partitions(stream, sector, size))
    if read_at(stream, 510, 2) != b"\x55\xaa":
        return None, sector_size, whole
    try:
        stream.seek(0)
        mbr = MBR(stream, sector_size=sector_size)
    except DiskError:
        return None, sector_size, whole
    except (EOFError, RecursionError):
        raise Failure("MBR 扩展分区记录损坏。") from None
    if any(part.type == 0xEE for part in mbr.partitions):
        raise Failure("找到了保护性 MBR，但 GPT 表头缺失。")
    entries = [(part.number, part.offset, part.size, "") for part in mbr.partitions]
    return ("MBR", sector_size, entries) if entries else (None, sector_size, whole)


def volume_info(stream, number, offset, size, name):
    kind = encryption(stream)
    methods = []
    issue = None
    try:
        if kind == "bitlocker":
            stream.seek(0)
            volume = BDE(stream)
            if volume.has_recovery_password():
                methods.append("recovery_password")
            if volume.has_passphrase():
                methods.append("password")
            if volume.has_bek():
                methods.append("key_file")
            if not methods:
                issue = "此卷未提供密码、恢复密钥或 BEK 密钥保护器。"
        elif kind.startswith("luks"):
            stream.seek(0)
            if LUKS(stream).keyslots:
                methods = ["password", "key_file"]
            else:
                issue = "此卷没有可用的 LUKS 密钥槽。"
    except Exception:
        issue = "无法读取加密卷的元数据。"
        methods = []
    return dict(number=number, offset=offset, size=size, name=name, kind=kind, methods=methods, issue=issue)


def unlock(stream, kind, credential):
    stream.seek(0)
    volume = BDE(stream) if kind == "bitlocker" else LUKS(stream)
    method, value = credential["type"], credential["value"]
    try:
        if method == "key_file":
            path = Path(value)
            if not 0 < file_info(path).st_size <= 8 * CHUNK:
                raise Failure("密钥文件必须为 1 字节至 8 MiB。")
            with path.open("rb") as key:
                if kind == "bitlocker":
                    volume.unlock_with_bek(key)
                else:
                    volume.unlock_with_key_fh(key, size=8 * CHUNK)
        elif method == "recovery_password":
            digits = value.replace("-", "")
            if len(digits) != 48 or not digits.isascii() or not digits.isdigit():
                raise Failure("请输入 BitLocker 的 48 位数字恢复密钥。")
            volume.unlock_with_recovery_password("-".join(digits[i : i + 6] for i in range(0, 48, 6)))
        else:
            volume.unlock_with_passphrase(value)
    except (ValueError, EOFError, TypeError):
        raise Failure("此凭据无法解锁所选卷。请检查密码、恢复密钥或密钥文件。") from None
    return volume.open()


def write_volume(decoded, maximum, output):
    size = decoded.size
    if not size or not 0 < size <= maximum or size % decoded.sector_size:
        raise Failure("解密后的卷长度无效或镜像已截断。")
    digest = hashlib.sha256()
    done = 0
    updated = time.monotonic()
    emit("progress", dict(done=done, total=size))
    with output.open("wb") as file:
        while done < size:
            block = decoded.read(min(CHUNK, size - done))
            if not block:
                raise Failure("卷数据提前结束。请检查镜像是否完整。")
            file.write(block)
            digest.update(block)
            done += len(block)
            now = time.monotonic()
            if now - updated >= 0.2:
                emit("progress", dict(done=done, total=size))
                updated = now
        file.flush()
        os.fsync(file.fileno())
    return dict(bytes_written=done, sha256=digest.hexdigest())


def main(request):
    if request["operation"] == "probe":
        emit("ready")
        return
    path = Path(request["path"])
    with ExitStack() as files:
        stream, size, hint, container, paths = open_image(path, files)
        stamp = file_stamp(paths)
        table, sector_size, entries = partitions(stream, size, hint)
        volumes = []
        for number, offset, length, name in entries:
            if offset < 0 or length <= 0 or offset + length > size:
                raise Failure("分区范围超出镜像。请检查镜像是否完整。")
            view = RangeStream(stream, offset, length)
            volumes.append(volume_info(view, number, offset, length, name))

        if request["operation"] == "inspect":
            if stamp != file_stamp(paths):
                raise Failure("读取期间镜像发生变化，请重新检查。")
            emit("inventory", dict(container=container, table=table, size=size, sector_size=sector_size,
                                   segments=len(paths), stamp=stamp, volumes=volumes))
            return

        selected = request["volume"]
        credential = request["credential"]
        if stamp != request["stamp"] or selected not in volumes:
            raise Failure("镜像或分段已发生变化，请重新检查。")
        if credential["type"] not in selected["methods"]:
            raise Failure("此加密卷不支持所选的凭据类型。")
        view = RangeStream(stream, selected["offset"], selected["size"])
        decoded = unlock(view, selected["kind"], credential)
        result = write_volume(decoded, selected["size"], Path(request["output"]))
        if stamp != file_stamp(paths):
            raise Failure("导出期间镜像发生变化，请重新检查。")
        emit("exported", result)


try:
    main(json.load(sys.stdin.buffer))
except Failure as error:
    emit("error", str(error))
except FileNotFoundError:
    emit("error", "找不到镜像、密钥文件或输出目录。")
except PermissionError:
    emit("error", "无法读取镜像或密钥文件，或无法写入输出目录。")
except OSError as error:
    emit("error", f"镜像读取或写入失败。请检查可用空间和文件状态（错误 {error.errno}）。")
except Exception:
    emit("error", "无法解析或解密此镜像。请核对镜像格式与完整性。")
