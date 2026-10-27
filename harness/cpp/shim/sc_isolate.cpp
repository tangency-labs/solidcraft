// SPDX-License-Identifier: Apache-2.0
// Per-case isolation of the C++ library's process-global state (solidcore_ffi::isolate).
//
// Upstream keeps state in function statics that the first call in a process fixes (ND-3:
// SmFilletSolver::ComputePointValues' sdStaticStep). A case whose C++ side runs in a
// fresh forked child starts from the parent's state, which never ran the case code, and
// whatever the case latches dies with the child. The Rust wrapper runs the case's shim
// calls in the child, sends their result back through the pipe and calls
// sc_isolate_exit; the parent reads the pipe to its end and then sc_isolate_wait.
//
//  * sc_isolate_fork(fd): forks while holding the shared native mutex, so no other thread
//    is inside native code at the fork. The child cannot use that copy: a recursive
//    mutex records its owner's kernel thread id, which differs in the child, so the
//    child's first SC_TRY would block forever. The child, single-threaded, constructs a
//    fresh (unlocked) mutex in its place. Returns the child's pid in the parent (*fd = the
//    read end of the pipe), 0 in the child (*fd = the write end), or -1 when the pipe or
//    fork fails (*fd = -1; the caller then runs the case in-process).
//  * sc_isolate_alarm(seconds): in the child, replaces the 600 s alarm with one of
//    `seconds` (1 to 600; others leave it), for a case whose native call may never end.
//  * sc_isolate_exit(): ends the child (_exit(0): no atexit handlers, no stdio flush).
//  * sc_isolate_wait(pid): 1 when the child exited with status 0, else 0 (killed by a
//    signal, the 600 s alarm, or another exit).
#include "sc_shim.h"

#include <new>

#ifndef _WIN32
#include <cerrno>
#include <cstdio>
#include <signal.h>
#include <sys/wait.h>
#include <unistd.h>
#ifdef __linux__
#include <sys/prctl.h>
#endif

SC_EXPORT int64_t sc_isolate_fork(int32_t* fd)
{
    if (fd == nullptr)
        return -1;
    *fd = -1;
    int fds[2];
    std::recursive_mutex& mutex = sc_native_mutex();
    mutex.lock();
    if (pipe(fds) != 0)
    {
        mutex.unlock();
        return -1;
    }
    fflush(stdout);
    fflush(stderr);
    const pid_t pid = fork();
    if (pid < 0)
    {
        close(fds[0]);
        close(fds[1]);
        mutex.unlock();
        return -1;
    }
    if (pid == 0)
    {
#ifdef __linux__
        // (no core dump of an expected native crash)
        prctl(PR_SET_DUMPABLE, 0);
#endif
        // A child that writes into a closed pipe dies instead of hanging.
        signal(SIGPIPE, SIG_DFL);
        // (the inherited lock names the parent's thread id; see above)
        new (&mutex) std::recursive_mutex();
        alarm(600);
        close(fds[0]);
        *fd = fds[1];
        return 0;
    }
    close(fds[1]);
    mutex.unlock();
    *fd = fds[0];
    return int64_t(pid);
}

SC_EXPORT void sc_isolate_alarm(uint32_t seconds)
{
    if (seconds >= 1 && seconds <= 600)
        alarm(seconds);
}

SC_EXPORT void sc_isolate_exit() { _exit(0); }

SC_EXPORT int32_t sc_isolate_wait(int64_t pid)
{
    int ws = 0;
    pid_t w;
    do
        w = waitpid(pid_t(pid), &ws, 0);
    while (w < 0 && errno == EINTR);
    return (w == pid_t(pid) && WIFEXITED(ws) && WEXITSTATUS(ws) == 0) ? 1 : 0;
}
#endif
