// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.global_solver`. Wraps upstream SmGlobalSolver
// (source/SMLib/src/SmGlobalSolver.cpp, source/SMLib/inc/SmGlobalSolver.h).
//
// sc_geom_global_solver_{ints,floats}(prog, n, out, cap, out_len) run the same flat double
// program against up to three hand-built trees, one solver (an SmGlobalSolver subclass that
// only exposes the protected members) and one solution array, and return the integer or
// float output stream. The encoding is shared with
// crates/solidcore-ffi/src/units/geom_global_solver.rs, which checks the static shape first;
// the checks that need run-time state (tree shapes, solver tree counts) are made here
// (SC_ERR_ARG) so that no path upstream would dereference a null or uninitialised pointer.
//
// Ops (op code, operands):
//   1 n [parent c1 c2 aux xmin ymin zmin xmax ymax zmax]*n
//                         append a tree (max 3) of n >= 1 nodes, node 0 = top; children
//                         both -1 or both indices > the node's own; aux = SmAuxDataType
//   2 kind nvars          solver: 0 SmGlobalSolver(), 1 (t0, nvars), 2 (t0, t1, nvars),
//                         3 = 2 plus m_apTrees[2] = t2 and SetNumTrees(3); then opt vectors
//                         = NULL and SetSolutions(&array)
//   3 op req tol best cat atdist v0 v1 v2 (9)
//                         SetSolverOperation, SetSolutionRequested, Set3DTolerance,
//                         SetBestAnswerSoFarSq, SetOperationCategory, SetAtDistance, opt vectors
//   4 op req tol best v0 v1 v2 (9)
//                         SolveIt(..., array)            ints: status
//   5 key combine S       AddSortedSolution(S, key, combine)
//   6                     dump: ints category, projected, size, per solution type nvars
//                         nobjs objects[5] nodes[5]; floats best_sq, at_distance_sq, tol,
//                         per solution start value + 10 params, end value + 10 params
//   7 b0 b1 b2            BranchMayContainAnswers(branch)    ints: 0/1
//   8 b0 b1 b2            AreReadyForLocalSolve, FindBestSubdivisionNode
//                         ints: ready, status [, index, swap]
//   9 S S                 IdenticalSolutions     ints: pair type, preferred (0 none, 1, 2)
//  10                     array ReSet
// A solution `S` is 35 doubles: type nvars nobjs, start value + 10 params, end value + 10
// params, 5 objects, 5 nodes (ignored: NULL). An object is -1 (NULL), 0..99 (an SmLine) or
// 1400..1499 (an SmObject stand-in that is no kind of anything). Branch slot b_i is a node
// index of tree i or -1 (NULL). Output nodes are the node index in tree i for slot i
// (-1 NULL, -2 not a node of tree i).
#include "sc_shim.h"

#include <SmGlobalSolver.h>
#include <SmLine.h>
#include <SmSolutionArray.h>
#include <SmTree.h>

#include <map>
#include <memory>
#include <vector>

namespace
{
class ScStandIn : public SmObject
{
public:
    ScStandIn() {}
    SmBoolean IsKindOf(SM_TYPE) const override { return FALSE; }
};

const SmContext* const kNoContext = nullptr;

const SmContext* the_context()
{
    static SmContext* p = new SmContext();
    return p;
}

// Objects live for the whole process.
SmObject* object(int64_t code)
{
    static std::map<int64_t, SmObject*> objs;
    if (code < 0)
        return nullptr;
    auto it = objs.find(code);
    if (it != objs.end())
        return it->second;
    SmObject* o = nullptr;
    if (code / 100 == 0)
        o = new (the_context()) SmLine(SmPoint3d(0.0, 0.0, 0.0), SmPoint3d(1.0, 2.0, 3.0), 3, the_context());
    else
        o = new (kNoContext) ScStandIn();
    objs[code] = o;
    return o;
}

std::map<const SmObject*, int64_t>& object_codes()
{
    static std::map<const SmObject*, int64_t> m;
    return m;
}

SmObject* obj(double d)
{
    int64_t code = (int64_t)d;
    SmObject* o = object(code);
    if (o)
        object_codes()[o] = code;
    return o;
}

int64_t obj_code(const SmObject* o)
{
    if (o == nullptr)
        return -1;
    auto it = object_codes().find(o);
    return it == object_codes().end() ? -2 : it->second;
}

int kind_of(double d) { return d < 0 ? -1 : (int)((int64_t)d / 100); }

const int64_t kSol = 35;

SmSolution sol(const double* a)
{
    SmSolution s;
    s.m_eSolutionType = (SmSolutionType)(int)a[0];
    s.m_lNumVariables = (ULONG)a[1];
    s.m_lNumObjects = (ULONG)a[2];
    s.m_vStart.m_dSolutionValue = a[3];
    for (int i = 0; i < SM_SA_MAX_TREES * 2; i++)
        s.m_vStart[i] = a[4 + i];
    s.m_vEnd.m_dSolutionValue = a[14];
    for (int i = 0; i < SM_SA_MAX_TREES * 2; i++)
        s.m_vEnd[i] = a[15 + i];
    for (int i = 0; i < SM_SA_MAX_TREES; i++)
    {
        s.m_apObjects[i] = obj(a[25 + i]);
        s.m_apNodes[i] = nullptr;
    }
    return s;
}

// GetParameterInterval dereferences the object GetObjectIndexForParameterIndex returns; that
// search can stop on a NULL slot when the first object is a curve.
bool interval_safe(const double* s)
{
    if (!(s[2] > 0 && kind_of(s[25]) == 0))
        return true;
    for (int i = 0; i < (int)s[2]; i++)
        if (s[25 + i] < 0)
            return false;
    return true;
}

class ScTree : public SmTree
{
public:
    ScTree() : SmTree((SmObject*)nullptr) {}
    void set_top(SmTreeNode* n) { m_pTopNode = n; }
};

struct Tree
{
    std::unique_ptr<ScTree> tree;
    std::vector<SmTreeNode> nodes;
};

class ScSolver : public SmGlobalSolver
{
public:
    ScSolver() : SmGlobalSolver() {}
    ScSolver(SmTree* a, ULONG n) : SmGlobalSolver(a, n) {}
    ScSolver(SmTree* a, SmTree* b, ULONG n) : SmGlobalSolver(a, b, n) {}
    void set_tree(int i, SmTree* t) { m_apTrees[i] = t; }
    void set_opt(const SmVector3d* v) { m_cpOptVectors = v; }
    ULONG num_trees() const { return m_lNumTrees; }
    SmSolverOperationType op() const { return m_eSolverOperation; }
    double best_sq() const { return m_dBestAnswerSoFarSq; }
    double at_sq() const { return m_dAtDistanceSq; }
    double tol() const { return m_d3dTolerance; }
    int category() const { return (int)m_eOperationCategory; }
    int projected() const { return m_bProjectedOperation ? 1 : 0; }
    SmBoolean may(SmTreeNode** b) { return BranchMayContainAnswers(b); }
    SmBoolean ready(SmTreeNode** b) const { return AreReadyForLocalSolve(b); }
    SmStatus best(SmTreeNode** b, ULONG& i, SmBoolean& s) const { return FindBestSubdivisionNode(b, i, s); }
    SmSolutionPairType ident(const SmSolution& a, const SmSolution& b, const SmSolution*& p) const
    {
        return IdenticalSolutions(a, b, p);
    }
};

// Operations whose culling or subdivision reads the second branch slot.
bool two_tree_op(int op)
{
    switch ((SmSolverOperationType)op)
    {
    case SM_SO_MINIMIZE:
    case SM_SO_MAXIMIZE:
    case SM_SO_AT_DISTANCE:
    case SM_SO_PROJECTED_MINIMIZE:
    case SM_SO_PROJECTED_MAXIMIZE:
    case SM_SO_PROJECTED_INTERSECT:
    case SM_SO_ROTATED_PROJECTED_INTERSECT:
    case SM_SO_PERSPECTIVE_INTERSECT:
    case SM_SO_DIRECTED_MINIMIZE:
    case SM_SO_SIGNED_DIRECTED_MINIMIZE:
    case SM_SO_DIRECTED_MAXIMIZE:
    case SM_SO_SIGNED_PIVOT_MINIMIZE:
    case SM_SO_RAYFIRE:
    case SM_SO_3D_SIGNED_DIRECTED_MINIMIZE:
    case SM_SO_SURFACE_PROJECTED_INTERSECT: return true;
    default: return false;
    }
}

// The solve of `op` over the solver's trees never dereferences a null/uninitialised node.
bool solve_safe(int op, ULONG ntrees, const std::vector<Tree>& trees)
{
    if (ntrees > trees.size())
        return false;
    if (two_tree_op(op))
    {
        if (ntrees == 0 || ntrees > 2)
            return false;
        bool needs_second = op == SM_SO_RAYFIRE || op == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE;
        if (needs_second && ntrees != 2)
            return false;
        // RAYFIRE always subdivides the second tree's node: the first tree must stay a leaf
        if (op == SM_SO_RAYFIRE && trees[0].nodes.size() != 1)
            return false;
    }
    return true;
}

int64_t node_index(const std::vector<Tree>& trees, int slot, const SmTreeNode* p)
{
    if (p == nullptr)
        return -1;
    if (slot >= (int)trees.size())
        return -2;
    const std::vector<SmTreeNode>& v = trees[(size_t)slot].nodes;
    for (size_t k = 0; k < v.size(); k++)
        if (&v[k] == p)
            return (int64_t)k;
    return -2;
}

void put_end(std::vector<double>& of, const SmSolutionEnd& e)
{
    of.push_back(e.m_dSolutionValue);
    for (int i = 0; i < SM_SA_MAX_TREES * 2; i++)
        of.push_back(e[i]);
}

int64_t st(SmStatus s) { return s == SM_SUCCESS ? 0 : (int64_t)s; }

int64_t op_fixed(int op, const double* a, int64_t avail)
{
    switch (op)
    {
    case 1: return avail < 1 ? -1 : 1 + 10 * (int64_t)a[0];
    case 2: return 2;
    case 3: return 15;
    case 4: return 13;
    case 5: return 2 + kSol;
    case 6: return 0;
    case 7:
    case 8: return 3;
    case 9: return 2 * kSol;
    case 10: return 0;
    default: return -1;
    }
}

int32_t run(const double* p, int64_t n, std::vector<int64_t>& oi, std::vector<double>& of)
{
    std::vector<Tree> trees;
    std::unique_ptr<ScSolver> solver;
    SmSolutionArray arr;
    SmVector3d opt[3];
    bool opt_set = false;
    int64_t i = 0;
    while (i < n)
    {
        int op = (int)p[i];
        const double* a = p + i + 1;
        int64_t fixed = op_fixed(op, a, n - i - 1);
        if (fixed < 0 || i + 1 + fixed > n)
            return SC_ERR_ARG;
        switch (op)
        {
        case 1:
        {
            int64_t nn = (int64_t)a[0];
            if (trees.size() >= 3 || nn < 1 || solver)
                return SC_ERR_ARG;
            Tree t;
            t.tree.reset(new (kNoContext) ScTree());
            t.nodes.resize((size_t)nn);
            for (int64_t k = 0; k < nn; k++)
            {
                const double* d = a + 1 + 10 * k;
                int64_t par = (int64_t)d[0], c1 = (int64_t)d[1], c2 = (int64_t)d[2];
                if (par < -1 || par >= nn)
                    return SC_ERR_ARG;
                bool leaf = c1 == -1 && c2 == -1;
                bool inner = c1 > k && c1 < nn && c2 > k && c2 < nn;
                if (!leaf && !inner)
                    return SC_ERR_ARG;
                SmTreeNode& nd = t.nodes[(size_t)k];
                nd.m_pTree = t.tree.get();
                nd.m_pParent = par < 0 ? nullptr : &t.nodes[(size_t)par];
                nd.m_pChild1 = c1 < 0 ? nullptr : &t.nodes[(size_t)c1];
                nd.m_pChild2 = c2 < 0 ? nullptr : &t.nodes[(size_t)c2];
                nd.m_eAuxDataType = (SmAuxDataType)(int)d[3];
                nd.m_sBBox = SmExtent3d(d[4], d[5], d[6], d[7], d[8], d[9]);
            }
            t.tree->set_top(&t.nodes[0]);
            trees.push_back(std::move(t));
            break;
        }
        case 2:
        {
            int kind = (int)a[0];
            ULONG nv = (ULONG)a[1];
            if (solver || (size_t)(kind == 3 ? 3 : kind) > trees.size())
                return SC_ERR_ARG;
            if (kind == 0)
                solver.reset(new ScSolver());
            else if (kind == 1)
                solver.reset(new ScSolver(trees[0].tree.get(), nv));
            else
            {
                solver.reset(new ScSolver(trees[0].tree.get(), trees[1].tree.get(), nv));
                if (kind == 3)
                {
                    solver->set_tree(2, trees[2].tree.get());
                    solver->SetNumTrees(3);
                }
            }
            solver->set_opt(nullptr);
            solver->SetSolutions(&arr);
            break;
        }
        case 3:
            if (!solver)
                return SC_ERR_ARG;
            for (int k = 0; k < 3; k++)
                opt[k] = SmVector3d(a[6 + 3 * k], a[7 + 3 * k], a[8 + 3 * k]);
            solver->SetSolverOperation((SmSolverOperationType)(int)a[0]);
            solver->SetSolutionRequested((SmSolutionRequestedType)(int)a[1]);
            solver->Set3DTolerance(a[2]);
            solver->SetBestAnswerSoFarSq(a[3]);
            solver->SetOperationCategory((SmOperationCategory)(int)a[4]);
            solver->SetAtDistance(a[5]);
            solver->set_opt(opt);
            opt_set = true;
            break;
        case 4:
        {
            if (!solver || !solve_safe((int)a[0], solver->num_trees(), trees))
                return SC_ERR_ARG;
            for (int k = 0; k < 3; k++)
                opt[k] = SmVector3d(a[4 + 3 * k], a[5 + 3 * k], a[6 + 3 * k]);
            SmStatus r = solver->SolveIt((SmSolverOperationType)(int)a[0], (SmSolutionRequestedType)(int)a[1],
                                         a[2], a[3], opt, arr);
            oi.push_back(st(r));
            opt_set = true;
            break;
        }
        case 5:
            if (!solver || !interval_safe(a + 2))
                return SC_ERR_ARG;
            if (solver->op() == SM_SO_RAYFIRE)
            {
                // the RAYFIRE trim reads the zone tolerance of the first object of the first and
                // last solutions: keep those objects NULL
                if (a[2 + 2] > 0 && a[2 + 25] >= 0)
                    return SC_ERR_ARG;
                for (ULONG k = 0; k < arr.GetSize(); k++)
                    if (arr[k].m_lNumObjects > 0 && arr[k].m_apObjects[0] != nullptr)
                        return SC_ERR_ARG;
            }
            solver->AddSortedSolution(sol(a + 2), (SmSortKeyType)(int)a[0], a[1] != 0.0 ? TRUE : FALSE);
            break;
        case 6:
        {
            if (!solver)
                return SC_ERR_ARG;
            oi.push_back(solver->category());
            oi.push_back(solver->projected());
            oi.push_back((int64_t)arr.GetSize());
            of.push_back(solver->best_sq());
            of.push_back(solver->at_sq());
            of.push_back(solver->tol());
            for (ULONG k = 0; k < arr.GetSize(); k++)
            {
                const SmSolution& s = arr[k];
                oi.push_back((int64_t)s.m_eSolutionType);
                oi.push_back((int64_t)s.m_lNumVariables);
                oi.push_back((int64_t)s.m_lNumObjects);
                for (int j = 0; j < SM_SA_MAX_TREES; j++)
                    oi.push_back(obj_code(s.m_apObjects[j]));
                for (int j = 0; j < SM_SA_MAX_TREES; j++)
                    oi.push_back(node_index(trees, j, s.m_apNodes[j]));
                put_end(of, s.m_vStart);
                put_end(of, s.m_vEnd);
            }
            break;
        }
        case 7:
        case 8:
        {
            if (!solver)
                return SC_ERR_ARG;
            ULONG nt = solver->num_trees();
            int sop = (int)solver->op();
            SmTreeNode* br[SM_GS_MAX_TREES] = {nullptr, nullptr, nullptr};
            for (int k = 0; k < 3; k++)
            {
                int64_t idx = (int64_t)a[k];
                if (idx < -1 || (idx >= 0 && (k >= (int)trees.size() || idx >= (int64_t)trees[(size_t)k].nodes.size())))
                    return SC_ERR_ARG;
                br[k] = idx < 0 ? nullptr : &trees[(size_t)k].nodes[(size_t)idx];
            }
            for (ULONG k = 0; k < nt; k++)
                if (br[k] == nullptr)
                    return SC_ERR_ARG;
            if (nt > trees.size() || (two_tree_op(sop) && nt != 1 && nt != 2))
                return SC_ERR_ARG;
            if (!opt_set)
                return SC_ERR_ARG;
            if (op == 7)
            {
                oi.push_back(solver->may(br) ? 1 : 0);
            }
            else
            {
                if ((sop == SM_SO_RAYFIRE || sop == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE) && nt != 2)
                    return SC_ERR_ARG;
                if (sop == SM_SO_RAYFIRE && br[1]->m_pChild1 == nullptr)
                {
                    // only safe when no node is subdividable (status SM_ERR before the dereference)
                    for (ULONG k = 0; k < nt; k++)
                        if (br[k]->m_pChild1 != nullptr)
                            return SC_ERR_ARG;
                }
                oi.push_back(solver->ready(br) ? 1 : 0);
                ULONG bi = 0;
                SmBoolean sw = FALSE;
                SmStatus r = solver->best(br, bi, sw);
                oi.push_back(st(r));
                if (r == SM_SUCCESS)
                {
                    oi.push_back((int64_t)bi);
                    oi.push_back(sw ? 1 : 0);
                }
            }
            break;
        }
        case 9:
        {
            if (!solver || !interval_safe(a) || !interval_safe(a + kSol))
                return SC_ERR_ARG;
            SmSolution s1 = sol(a), s2 = sol(a + kSol);
            const SmSolution* pref = nullptr;
            SmSolutionPairType t = solver->ident(s1, s2, pref);
            oi.push_back((int64_t)t);
            oi.push_back(pref == nullptr ? 0 : pref == &s1 ? 1 : 2);
            break;
        }
        case 10: arr.ReSet(); break;
        default: return SC_ERR_ARG;
        }
        i += 1 + fixed;
    }
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_geom_global_solver_ints(const double* prog, int64_t n, int64_t* out, int64_t cap,
                                             int64_t* out_len)
{
    if (n < 0 || (n > 0 && prog == nullptr))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<int64_t> oi;
    std::vector<double> of;
    int32_t rc = run(prog, n, oi, of);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(oi.data(), (int64_t)oi.size(), out, cap, out_len);
    SC_CATCH
}

SC_EXPORT int32_t sc_geom_global_solver_floats(const double* prog, int64_t n, double* out, int64_t cap,
                                               int64_t* out_len)
{
    if (n < 0 || (n > 0 && prog == nullptr))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<int64_t> oi;
    std::vector<double> of;
    int32_t rc = run(prog, n, oi, of);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(of.data(), (int64_t)of.size(), out, cap, out_len);
    SC_CATCH
}
