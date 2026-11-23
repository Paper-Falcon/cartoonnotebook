"""创建 Windows 桌面快捷方式（.lnk）。

PowerShell 的 New-Object -ComObject 被策略拦截，这里直接用 ctypes 调 IShellLink。
用法：python scripts/make-shortcut.py <exe路径> <lnk路径> [描述]
"""

import ctypes
import os
import sys
import uuid
from ctypes import POINTER, byref, c_int, c_long, c_ubyte, c_ulong, c_ushort, c_void_p, c_wchar_p
from ctypes import wintypes


class GUID(ctypes.Structure):
    _fields_ = [
        ("Data1", c_ulong),
        ("Data2", c_ushort),
        ("Data3", c_ushort),
        ("Data4", c_ubyte * 8),
    ]


def make_guid(text: str) -> GUID:
    u = uuid.UUID(text)
    g = GUID()
    g.Data1 = u.time_low
    g.Data2 = u.time_mid
    g.Data3 = u.time_hi_version
    for i, b in enumerate(u.bytes[8:]):
        g.Data4[i] = b
    return g


CLSID_SHELLLINK = make_guid("{00021401-0000-0000-C000-000000000046}")
IID_ISHELLLINKW = make_guid("{000214F9-0000-0000-C000-000000000046}")
IID_IPERSISTFILE = make_guid("{0000010b-0000-0000-C000-000000000046}")

ole32 = ctypes.windll.ole32


def vfunc(ptr, index, *argtypes):
    vtable = ctypes.cast(ptr, POINTER(POINTER(c_void_p))).contents
    proto = ctypes.WINFUNCTYPE(c_long, c_void_p, *argtypes)
    return proto(vtable[index])


def read_shortcut(lnk: str) -> int:
    """载入 .lnk 并打印它指向的目标，用来验证快捷方式是否真的建对了。"""
    ole32.CoInitialize(None)
    link = c_void_p()
    hr = ole32.CoCreateInstance(
        byref(CLSID_SHELLLINK), None, 1, byref(IID_ISHELLLINKW), byref(link)
    )
    if hr != 0 or not link:
        print(f"CoCreateInstance 失败 hr={hr:#x}")
        return 1

    persist = c_void_p()
    vfunc(link.value, 0, POINTER(GUID), POINTER(c_void_p))(
        link, byref(IID_IPERSISTFILE), byref(persist)
    )
    # IPersistFile::Load 在下标 5
    hr = vfunc(persist.value, 5, c_wchar_p, wintypes.DWORD)(persist, lnk, 0)
    if hr != 0:
        print(f"Load 失败 hr={hr:#x}")
        return 1

    buf = ctypes.create_unicode_buffer(1024)
    vfunc(link.value, 3, c_wchar_p, c_int, c_void_p, wintypes.DWORD)(
        link, buf, 1024, None, 0
    )
    print(f"快捷方式指向: {buf.value}")
    print(f"目标存在: {os.path.isfile(buf.value)}")

    work = ctypes.create_unicode_buffer(1024)
    vfunc(link.value, 8, c_wchar_p, c_int)(link, work, 1024)
    print(f"工作目录: {work.value}")
    return 0


def main() -> int:
    if len(sys.argv) == 3 and sys.argv[1] == "--read":
        return read_shortcut(os.path.abspath(sys.argv[2]))
    if len(sys.argv) < 3:
        print("用法: make-shortcut.py <exe> <lnk> [描述]")
        print("      make-shortcut.py --read <lnk>")
        return 2

    exe = os.path.abspath(sys.argv[1])
    lnk = os.path.abspath(sys.argv[2])
    desc = sys.argv[3] if len(sys.argv) > 3 else os.path.splitext(os.path.basename(lnk))[0]

    ole32.CoInitialize(None)

    link = c_void_p()
    hr = ole32.CoCreateInstance(
        byref(CLSID_SHELLLINK), None, 1, byref(IID_ISHELLLINKW), byref(link)
    )
    if hr != 0 or not link:
        print(f"CoCreateInstance 失败 hr={hr:#x}")
        return 1

    # IShellLinkW 虚表下标：7 SetDescription / 9 SetWorkingDirectory / 17 SetIconLocation / 20 SetPath
    vfunc(link.value, 20, c_wchar_p)(link, exe)
    vfunc(link.value, 9, c_wchar_p)(link, os.path.dirname(exe))
    vfunc(link.value, 7, c_wchar_p)(link, desc)
    vfunc(link.value, 17, c_wchar_p, c_int)(link, f"{exe},0", 0)

    persist = c_void_p()
    hr = vfunc(link.value, 0, POINTER(GUID), POINTER(c_void_p))(
        link, byref(IID_IPERSISTFILE), byref(persist)
    )
    if hr != 0 or not persist:
        print(f"QueryInterface(IPersistFile) 失败 hr={hr:#x}")
        return 1

    # IPersistFile::Save 在下标 6
    hr = vfunc(persist.value, 6, c_wchar_p, wintypes.BOOL)(persist, lnk, True)
    if hr != 0:
        print(f"Save 失败 hr={hr:#x}")
        return 1

    print(f"已创建 {lnk}")
    print(f"指向 {exe}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
