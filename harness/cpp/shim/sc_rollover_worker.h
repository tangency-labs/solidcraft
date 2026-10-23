// SPDX-License-Identifier: Apache-2.0
// Fresh-process containment: never allocate SMLib objects in a forked image of
// the multithreaded Rust host. posix_spawn executes a dedicated worker first.
#ifndef SC_ROLLOVER_WORKER_H
#define SC_ROLLOVER_WORKER_H
#include "sc_shim.h"
#include <array>
#include <cerrno>
#include <cstddef>
#if defined(__linux__) || defined(__APPLE__)
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <spawn.h>
#include <sys/wait.h>
#include <unistd.h>
extern char** environ;
#endif
namespace sc_rollover {
constexpr size_t INPUT_LIMIT=70, OUTPUT_LIMIT=4096;
struct Request { int64_t operation,count;
#ifdef SC_ROLLOVER_FAULT_TEST
    int64_t fail_new,fail_calloc,signal;
#endif
};
struct Header { int64_t status,length; };
#ifdef SC_ROLLOVER_FAULT_TEST
inline int64_t fail_new=-1,fail_calloc=-1,test_signal=-1;
#endif
#if defined(__linux__) || defined(__APPLE__)
inline bool cloexec_pipe(int fds[2]) {
#if defined(__linux__)
    return ::pipe2(fds,O_CLOEXEC)==0;
#else
    if(::pipe(fds)!=0) return false;
    if(::fcntl(fds[0],F_SETFD,FD_CLOEXEC)==0 &&
       ::fcntl(fds[1],F_SETFD,FD_CLOEXEC)==0) return true;
    ::close(fds[0]);::close(fds[1]);
    return false;
#endif
}
inline int spawn_worker(pid_t* child,const char* path,
                        const posix_spawn_file_actions_t* actions,char* const argv[]) {
#if defined(__APPLE__)
    // Darwin has no atomic pipe2(O_CLOEXEC). Close every descriptor except the
    // explicit dup2 actions, including pipes another thread has just created
    // but has not yet marked CLOEXEC. No worker can inherit another's pipes.
    posix_spawnattr_t attributes;
    int error=::posix_spawnattr_init(&attributes);
    if(error!=0) return error;
    error=::posix_spawnattr_setflags(&attributes,POSIX_SPAWN_CLOEXEC_DEFAULT);
    if(error==0) error=::posix_spawn(child,path,actions,&attributes,argv,environ);
    ::posix_spawnattr_destroy(&attributes);
    return error;
#else
    return ::posix_spawn(child,path,actions,nullptr,argv,environ);
#endif
}
inline bool write_all(int fd, const void* data, size_t n) {
    const char* p=static_cast<const char*>(data);
    while(n) { ssize_t sent=::write(fd,p,n); if(sent<0 && errno==EINTR) continue;
        if(sent<=0) return false; p+=sent; n-=static_cast<size_t>(sent); }
    return true;
}
inline bool read_all(int fd, void* data, size_t n) {
    char* p=static_cast<char*>(data);
    while(n) {
        pollfd event{fd,POLLIN,0};
        int ready; do { ready=::poll(&event,1,30000); } while(ready<0 && errno==EINTR);
        if(ready<=0) return false;
        ssize_t got=::read(fd,p,n); if(got<0 && errno==EINTR) continue;
        if(got<=0) return false; p+=got; n-=static_cast<size_t>(got);
    }
    return true;
}
#endif
// One bounded request is written into a private pipe before spawn: no parent
// write can race a dying child and deliver SIGPIPE. Pipes are CLOEXEC; Darwin
// also closes all non-explicit descriptors at spawn to cover pipe/fcntl races.
inline int32_t isolated(int64_t operation,const double* input,int64_t count,
                        double* out,int64_t cap,int64_t* len) {
#if (defined(__linux__) || defined(__APPLE__)) && defined(SC_ROLLOVER_WORKER_PATH)
    if(count<0 || count>static_cast<int64_t>(INPUT_LIMIT)) return SC_ERR_ARG;
    int in[2],result[2];
    if(!cloexec_pipe(in)) return SC_ERR_EXCEPTION;
    if(!cloexec_pipe(result)) {::close(in[0]);::close(in[1]);return SC_ERR_EXCEPTION;}
    Request request{operation,count
#ifdef SC_ROLLOVER_FAULT_TEST
        ,fail_new,fail_calloc,test_signal
#endif
    };
    bool okay=write_all(in[1],&request,sizeof(request)) &&
        write_all(in[1],input,static_cast<size_t>(count)*sizeof(double));
    ::close(in[1]);
    posix_spawn_file_actions_t actions;
    if(::posix_spawn_file_actions_init(&actions)!=0) {
        ::close(in[0]);::close(result[0]);::close(result[1]);return SC_ERR_EXCEPTION;
    }
    okay=okay && ::posix_spawn_file_actions_adddup2(&actions,in[0],STDIN_FILENO)==0;
    okay=okay && ::posix_spawn_file_actions_adddup2(&actions,result[1],STDOUT_FILENO)==0;
    const char* path=SC_ROLLOVER_WORKER_PATH;
    char* argv[]={const_cast<char*>(path),const_cast<char*>("--rollover-worker"),nullptr};
    pid_t child=-1;
    int error=okay?spawn_worker(&child,path,&actions,argv):-1;
    ::posix_spawn_file_actions_destroy(&actions);
    ::close(in[0]);::close(result[1]);
    if(error!=0) {::close(result[0]);return SC_ERR_EXCEPTION;}
    Header header{SC_ERR_EXCEPTION,0};
    std::array<double,OUTPUT_LIMIT> buffer{};
    okay=read_all(result[0],&header,sizeof(header));
    if(okay && (header.length<0 || header.length>static_cast<int64_t>(OUTPUT_LIMIT))) okay=false;
    if(okay && header.status==SC_OK)
        okay=read_all(result[0],buffer.data(),static_cast<size_t>(header.length)*sizeof(double));
    ::close(result[0]);
    if(!okay) ::kill(child,SIGKILL); // only our own failed/stalled worker
    int status=0;pid_t reaped;
    do {reaped=::waitpid(child,&status,0);} while(reaped<0 && errno==EINTR);
    if(!okay || reaped!=child || !WIFEXITED(status) || WEXITSTATUS(status)!=0) return SC_ERR_EXCEPTION;
    if(header.status!=SC_OK) return static_cast<int32_t>(header.status);
    return sc_copy_out(buffer.data(),header.length,out,cap,len);
#else
    (void)operation;(void)input;(void)count;(void)out;(void)cap;(void)len;
    return SC_ERR_EXCEPTION;
#endif
}
}
#endif
