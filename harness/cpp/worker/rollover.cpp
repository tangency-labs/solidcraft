// SPDX-License-Identifier: Apache-2.0
// Compiled separately by solidcore-ffi/build.rs; runs after exec in a fresh image.
#include <cstdlib>
#include "../shim/blend_fillet_solver_p5.cpp"
static int rollover_worker_main() {
#if defined(__linux__)
    sc_rollover::Request request{};
    std::array<double,sc_rollover::INPUT_LIMIT> input{};
    if(!sc_rollover::read_all(STDIN_FILENO,&request,sizeof(request)) ||
       request.count<0 || request.count>static_cast<int64_t>(input.size()) ||
       !sc_rollover::read_all(STDIN_FILENO,input.data(),request.count*sizeof(double))) return 2;
#ifdef SC_ROLLOVER_FAULT_TEST
    if(request.signal==2) test_exit_trap=true;
    else if(request.signal>=0) ::raise(request.signal==0?SIGSEGV:SIGABRT);
    new_failure=request.fail_new;calloc_failure=request.fail_calloc;
#endif
    std::array<double,sc_rollover::OUTPUT_LIMIT> output{};
    sc_rollover::Header header{SC_ERR_EXCEPTION,0};
    if(request.operation==0)
        header.status=rollover_eval_in_process(input.data(),request.count,output.data(),output.size(),&header.length);
    else if(request.operation==1 && request.count==0)
        header.status=rollover_target_in_process(output.data(),output.size(),&header.length);
    if(header.length<0 || header.length>static_cast<int64_t>(output.size())) return 2;
    bool okay=sc_rollover::write_all(STDOUT_FILENO,&header,sizeof(header));
    if(okay && header.status==SC_OK)
        okay=sc_rollover::write_all(STDOUT_FILENO,output.data(),header.length*sizeof(double));
    return okay?0:2;
#else
    return 2;
#endif
}
#ifndef SC_ROLLOVER_FAULT_TEST
// The in-process functions already destroyed every fixture object before
// producing the reply. Skip global SMLib finalizers after the reply: waitpid
// must not wait on more unbounded C++ work outside the protocol read timeout.
int main() { std::_Exit(rollover_worker_main()); }
#endif
