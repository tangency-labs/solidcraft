#!/usr/bin/env python3
"""Poor man's sampling profiler: ptrace + frame-pointer unwinding (x86_64 Linux).

No perf_event needed (perf_event_paranoid=2 blocks samply). The target is our own
child (PTRACE_TRACEME), so yama ptrace_scope=1 allows it. Build the target with
`-C force-frame-pointers=yes` and debug line tables.

usage: sampler.py OUT.json INTERVAL_MS -- cmd args...
Writes {"maps": [...], "samples": [[addr, ...], ...]} (innermost first).
"""
import ctypes
import json
import os
import signal
import struct
import sys
import time

libc = ctypes.CDLL(None, use_errno=True)
libc.ptrace.restype = ctypes.c_long
libc.ptrace.argtypes = [ctypes.c_long, ctypes.c_long, ctypes.c_void_p, ctypes.c_void_p]
PTRACE_TRACEME, PTRACE_CONT, PTRACE_GETREGS, PTRACE_SETOPTIONS = 0, 7, 12, 0x4200
PTRACE_O_EXITKILL = 0x100000


class Regs(ctypes.Structure):
    _fields_ = [(n, ctypes.c_ulonglong) for n in (
        "r15 r14 r13 r12 rbp rbx r11 r10 r9 r8 rax rcx rdx rsi rdi orig_rax rip cs "
        "eflags rsp ss fs_base gs_base ds es fs gs").split()]


def main():
    out, interval = sys.argv[1], float(sys.argv[2]) / 1000.0
    cmd = sys.argv[sys.argv.index("--") + 1:]
    pid = os.fork()
    if pid == 0:
        libc.ptrace(PTRACE_TRACEME, 0, None, None)
        os.execvp(cmd[0], cmd)
    _, st = os.waitpid(pid, 0)  # exec SIGTRAP stop
    libc.ptrace(PTRACE_SETOPTIONS, pid, None, ctypes.c_void_p(PTRACE_O_EXITKILL))
    libc.ptrace(PTRACE_CONT, pid, None, None)
    mem = os.open(f"/proc/{pid}/mem", os.O_RDONLY)
    samples, maps = [], None
    regs = Regs()
    while True:
        time.sleep(interval)
        try:
            os.kill(pid, signal.SIGSTOP)
        except ProcessLookupError:
            break
        # Wait for our SIGSTOP; forward any other signal.
        done = False
        while True:
            _, st = os.waitpid(pid, 0)
            if os.WIFEXITED(st) or os.WIFSIGNALED(st):
                done = True
                break
            sig = os.WSTOPSIG(st)
            if sig == signal.SIGSTOP:
                break
            libc.ptrace(PTRACE_CONT, pid, None, ctypes.c_void_p(sig))
        if done:
            break
        if maps is None:
            with open(f"/proc/{pid}/maps") as f:
                maps = f.read()
        libc.ptrace(PTRACE_GETREGS, pid, None, ctypes.byref(regs))
        stack = [regs.rip]
        fp = regs.rbp
        for _ in range(512):
            if fp == 0 or fp & 7 or fp >= (1 << 47):
                break
            try:
                b = os.pread(mem, 16, fp)
            except OSError:
                break
            if len(b) < 16:
                break
            nfp, ret = struct.unpack("<QQ", b)
            if ret == 0:
                break
            stack.append(ret)
            if nfp <= fp:
                break
            fp = nfp
        samples.append(stack)
        libc.ptrace(PTRACE_CONT, pid, None, None)
    with open(out, "w") as f:
        json.dump({"maps": maps or "", "samples": samples, "exe": cmd[0]}, f)
    print(f"sampler: {len(samples)} samples -> {out}", file=sys.stderr)


main()
