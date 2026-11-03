// SPDX-License-Identifier: Apache-2.0
// Native WIP contract gate; compile with -I<3843ddb overlay>, no setback object.
// This includes only the allowed SMLib source to exercise its private row provider.
#include "SmBlendSetback.cpp"
#include <cstdio>

static void add_patch(sm_SbSectorRows& rows) {
    std::vector<std::vector<sm_SbT>> pc0(3), pcf(3);
    for (int k=0;k<3;++k) {
        sm_SbT zero=sm_SbTMake(0,0), free=sm_SbTMake(0,0);
        zero.c[0]=k==1?0:-1;
        free.c[0]=k==1?0:1;
        pc0[k].push_back(zero); pcf[k].push_back(free);
    }
    rows.AddPatch({0.0, SM_PI/2.0, SM_PI},1.0,1.0,pc0,pcf);
}
int main() {
    sm_SbSectorRows rows(1); add_patch(rows); add_patch(rows);
    if(rows.Size()!=8) return 1;
    double c=0,b=0;
    for(size_t slot=0;slot<8;++slot) {
        bool present=rows.Row(slot,&c,b);
        if(present!=(slot%2==0)) return 2;
        if(present && (c!=1.0 || b!=1.0+1e-12)) return 3;
    }
    std::vector<char> in(8,0);
    std::vector<std::pair<double,size_t>> violated;
    rows.Violated({0.0},1e-13,in,violated);
    std::sort(violated.begin(),violated.end());
    if(violated.size()!=4) return 4;
    for(size_t i=0;i<4;++i)
        if(violated[i].second!=2*i || violated[i].first!=-(1.0+1e-12)) return 5;
    // Taking the first tied row must leave the following stable slot identities.
    in[0]=1; rows.Violated({0.0},1e-13,in,violated);
    if(violated.size()!=3 || violated[0].second!=2 || violated[1].second!=4 || violated[2].second!=6) return 6;
    std::fill(in.begin(),in.end(),0);
    auto before=rows.m_lRowsBuilt;
    rows.Violated({2.0},1e-13,in,violated);
    // Four satisfied genuine rows are certified by the cheap bound; only the
    // four zero-height slots reach exact construction, which rejects them.
    if(!violated.empty() || rows.m_lRowsBuilt-before!=4) return 7;
    unsigned long long steps=0,peak=0; bool proven=false;
    std::vector<double> streamed,dense;
    auto status=sm_SbDualQPRows({1.0},{0.0},rows,1,1e-13,streamed,&proven,&steps,&peak);
    auto reference=sm_SbDualQP({1.0},{0.0},{1.0,1.0,1.0,1.0},
                              {1.0+1e-12,1.0+1e-12,1.0+1e-12,1.0+1e-12},
                              1,4,1e-13,dense);
    printf("slots %zu rows 4 peak %llu steps %llu value %.17g\n",
           rows.Size(),peak,steps,streamed.empty()?0:streamed[0]);
    if(status!=SM_SUCCESS || reference!=SM_SUCCESS || streamed!=dense ||
       peak!=4 || steps!=3 || proven) return 8;
    // Distinct right-hand sides identify piece/coefficient decoding independently
    // of the equal-slack test above. Two pieces, two coefficients, four pairs.
    sm_SbSectorRows grid(1);
    std::vector<std::vector<sm_SbT>> pc0(3),pcf(3);
    for(int k=0;k<3;++k) for(int piece=0;piece<2;++piece) {
        auto a=sm_SbTMake(0,1), f=sm_SbTMake(0,1);
        for(int i=0;i<2;++i) { a.c[i]=k==1?0:-(1+2*piece+i); f.c[i]=k==1?0:1; }
        pc0[k].push_back(a); pcf[k].push_back(f);
    }
    grid.AddPatch({0.0,SM_PI/2.0,SM_PI},1.0,1.0,pc0,pcf);
    if(grid.Size()!=16) return 9;
    for(size_t slot=0;slot<16;++slot) {
        bool present=grid.Row(slot,&c,b);
        if(present!=(slot%2==0) || (present && b!=double(1+slot/4)+1e-12)) return 10;
    }
    // More than one block of equally violated rows: keep exactly the first 2000.
    sm_SbSectorRows blocked(1);
    for(int patch=0;patch<1025;++patch) add_patch(blocked);
    steps=peak=0;
    status=sm_SbDualQPRows({1.0},{0.0},blocked,1,1e-13,streamed,&proven,&steps,&peak);
    if(status!=SM_SUCCESS || streamed!=dense || peak!=2000 || steps!=3) return 11;
    puts("PASS native streamed row slots, skipped rows, exact slack ties, cheap bound and dense QP equivalence");
}
