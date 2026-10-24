// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.database_io`.
// Wraps upstream SmDatabaseIO / SmDatabaseIOFile / SmDatabaseIOStream read side
// (source/SMLib/src/SmDatabaseIO.cpp) and the std::istream extraction the ASCII
// readers perform through SmDatabaseIO::GetInStreamPtr().
//
// Statuses are normalised: SM_SUCCESS -> 0.
// sc_topology_database_io_run runs a script of (op, arg) pairs against one reader
// and records, per op, [status, rdstate (-1 = no stream), read position (-1 = no
// stream), n_ints, n_dbls] plus the
// produced integers / doubles (appended to the flat output buffers).
#include "sc_shim.h"

#include <SmDatabaseIO.h>

#include <cstring>
#include <memory>
#include <iomanip>
#include <sstream>
#include <string>
#include <vector>

namespace
{
enum Op : int64_t
{
    OP_INFO = 0,
    OP_SET_VERSION = 1,
    OP_SET_BYTE_SWAP = 2,
    OP_BEGIN_READING = 3,
    OP_END_READING = 4,
    OP_READ_LONG = 5,
    OP_READ_ULONG = 6,
    OP_READ_LONGS = 7,
    OP_READ_DOUBLE = 8,
    OP_READ_DOUBLES = 9,
    OP_READ_BOOLEAN = 10,
    OP_READ_BOOLEANS = 11,
    OP_READ_SHORT = 12,
    OP_READ_CHAR = 13,
    OP_READ_TYPE = 14,
    OP_GO_TO_NEXT_LINE = 15,
    OP_EXTRACT_WORD = 16,
    OP_EXTRACT_LONG = 17,
    OP_EXTRACT_ULONG = 18,
    OP_EXTRACT_INT = 19,
    OP_EXTRACT_SHORT = 20,
    OP_EXTRACT_DOUBLE = 21,
    OP_GET = 22,
    OP_UNGET = 23,
    OP_SWAP = 24,
    OP_CLEAR = 25,
};

// Status recorded for a binary read that left the stream failed (upstream returns
// SM_SUCCESS with an uninitialised value; the Rust port returns an error).
const int64_t ST_SHORT = -2;

// Values a failed extraction leaves untouched (shared with the Rust runner).
const long SENT_LONG = -777777;
const unsigned long SENT_ULONG = 888888;
const int SENT_INT = -777777;
const short SENT_SHORT = -7777;
const double SENT_DOUBLE = -7777.25;

struct Out
{
    std::vector<int64_t> ints;
    std::vector<double> dbls;
};

int64_t state_of(SmDatabaseIO& db)
{
    std::istream* in = db.GetInStreamPtr();
    return in ? (int64_t)in->rdstate() : -1;
}

// Read position whatever the stream state (tellg() would give -1 after a failure).
int64_t position_of(SmDatabaseIO& db)
{
    std::istream* in = db.GetInStreamPtr();
    if (in == nullptr) return -1;
    return (int64_t)in->rdbuf()->pubseekoff(0, std::ios_base::cur, std::ios_base::in);
}

bool failed(SmDatabaseIO& db)
{
    std::istream* in = db.GetInStreamPtr();
    return in == nullptr || in->fail();
}

// Runs one op; returns its status.
int64_t run_op(SmDatabaseIO& db, int64_t op, int64_t arg, size_t word_cap, Out& o)
{
    switch (op)
    {
    case OP_INFO:
        o.ints.push_back((int64_t)db.GetFileType());
        o.ints.push_back(db.GetByteSwap() ? 1 : 0);
        o.ints.push_back((int64_t)db.GetVersionNum());
        o.ints.push_back(SmDatabaseIO::IsBigEndian() ? 1 : 0);
        return 0;
    case OP_SET_VERSION:
        db.SetVersionNum((ULONG)arg);
        return 0;
    case OP_SET_BYTE_SWAP:
        db.SetByteSwap(arg != 0);
        return 0;
    case OP_BEGIN_READING:
        return db.BeginReading();
    case OP_END_READING:
        return db.EndReading();
    case OP_READ_LONG:
    {
        long v = 0;
        SmStatus st = db.ReadLong(v);
        if (st != SM_SUCCESS) return st;
        if (failed(db)) return ST_SHORT;
        o.ints.push_back((int64_t)v);
        return 0;
    }
    case OP_READ_ULONG:
    {
        ULONG v = 0;
        SmStatus st = db.ReadLong(v);
        if (st != SM_SUCCESS) return st;
        if (failed(db)) return ST_SHORT;
        o.ints.push_back((int64_t)v);
        return 0;
    }
    case OP_READ_LONGS:
    {
        std::vector<ULONG> v((size_t)arg + 1, 0);
        SmStatus st = db.ReadLongs(v.data(), (ULONG)arg);
        if (st != SM_SUCCESS) return st;
        if (arg > 0 && failed(db)) return ST_SHORT;
        for (int64_t i = 0; i < arg; ++i)
            o.ints.push_back((int64_t)v[(size_t)i]);
        return 0;
    }
    case OP_READ_DOUBLE:
    {
        double v = 0.0;
        SmStatus st = db.ReadDouble(v);
        if (st != SM_SUCCESS) return st;
        if (failed(db)) return ST_SHORT;
        o.dbls.push_back(v);
        return 0;
    }
    case OP_READ_DOUBLES:
    {
        std::vector<double> v((size_t)arg + 1, 0.0);
        SmStatus st = db.ReadDoubles(v.data(), (ULONG)arg);
        if (st != SM_SUCCESS) return st;
        if (arg > 0 && failed(db)) return ST_SHORT;
        for (int64_t i = 0; i < arg; ++i)
            o.dbls.push_back(v[(size_t)i]);
        return 0;
    }
    case OP_READ_BOOLEAN:
    {
        SmBoolean v = 0;
        SmStatus st = db.ReadBoolean(v);
        if (st != SM_SUCCESS) return st;
        if (failed(db)) return ST_SHORT;
        o.ints.push_back((int64_t)v);
        return 0;
    }
    case OP_READ_BOOLEANS:
    {
        std::vector<SmBoolean> v((size_t)arg + 1, 0);
        SmStatus st = db.ReadBooleans(v.data(), (ULONG)arg);
        if (st != SM_SUCCESS) return st;
        if (arg > 0 && failed(db)) return ST_SHORT;
        for (int64_t i = 0; i < arg; ++i)
            o.ints.push_back((int64_t)v[(size_t)i]);
        return 0;
    }
    case OP_READ_SHORT:
    {
        short v = 0;
        SmStatus st = db.ReadShort(v);
        if (st != SM_SUCCESS) return st;
        if (failed(db)) return ST_SHORT;
        o.ints.push_back((int64_t)v);
        return 0;
    }
    case OP_READ_CHAR:
    {
        char v = 0;
        SmStatus st = db.ReadChar(v);
        if (st != SM_SUCCESS) return st;
        if (failed(db)) return ST_SHORT;
        o.ints.push_back((int64_t)(unsigned char)v);
        return 0;
    }
    case OP_READ_TYPE:
    {
        long t = SENT_LONG;
        ULONG dim = SENT_ULONG;
        bool ascii = db.GetFileType() == SM_ASCII;
        SmStatus st = db.ReadType(t, arg != 0 ? &dim : NULL);
        if (st != SM_SUCCESS) return st;
        if (!ascii && failed(db)) return ST_SHORT;
        o.ints.push_back((int64_t)t);
        if (arg != 0) o.ints.push_back((int64_t)dim);
        return 0;
    }
    case OP_GO_TO_NEXT_LINE:
        db.GoToNextLine();
        return 0;
    case OP_EXTRACT_WORD:
    {
        std::vector<char> buf(word_cap + 2, 0);
        // Bounded: word_cap comes from a length measured before the stream was
        // opened (and is 0 if that failed), so never trust it for an unbounded
        // read. setw(n) stores at most n-1 chars plus NUL; equal results for any
        // word within word_cap.
        *db.GetInStreamPtr() >> std::setw((std::streamsize)buf.size()) >> buf.data();
        for (size_t i = 0; buf[i] != 0; ++i)
            o.ints.push_back((int64_t)(unsigned char)buf[i]);
        return 0;
    }
    case OP_EXTRACT_LONG:
    {
        long v = SENT_LONG;
        *db.GetInStreamPtr() >> v;
        o.ints.push_back((int64_t)v);
        return 0;
    }
    case OP_EXTRACT_ULONG:
    {
        unsigned long v = SENT_ULONG;
        *db.GetInStreamPtr() >> v;
        o.ints.push_back((int64_t)v);
        return 0;
    }
    case OP_EXTRACT_INT:
    {
        int v = SENT_INT;
        *db.GetInStreamPtr() >> v;
        o.ints.push_back((int64_t)v);
        return 0;
    }
    case OP_EXTRACT_SHORT:
    {
        short v = SENT_SHORT;
        *db.GetInStreamPtr() >> v;
        o.ints.push_back((int64_t)v);
        return 0;
    }
    case OP_EXTRACT_DOUBLE:
    {
        double v = SENT_DOUBLE;
        *db.GetInStreamPtr() >> v;
        o.dbls.push_back(v);
        return 0;
    }
    case OP_GET:
        o.ints.push_back((int64_t)db.GetInStreamPtr()->get());
        return 0;
    case OP_UNGET:
        db.GetInStreamPtr()->unget();
        return 0;
    case OP_SWAP:
    {
        uint64_t u = (uint64_t)arg;
        double d;
        std::memcpy(&d, &u, sizeof d);
        o.ints.push_back((int64_t)db.ByteSwap16((unsigned short)u));
        o.ints.push_back((int64_t)SmDatabaseIO::ByteSwap32((std::uint32_t)u));
        o.ints.push_back((int64_t)db.ByteSwap32((unsigned long)u));
        o.ints.push_back((int64_t)db.ByteSwap64((unsigned long)u));
        o.ints.push_back((int64_t)db.ByteSwap64((unsigned long long)u));
        o.dbls.push_back(db.ByteSwap64(d));
        o.dbls.push_back(db.ByteSwap128(d));
        return 0;
    }
    case OP_CLEAR:
        db.GetInStreamPtr()->clear();
        return 0;
    default:
        return SC_ERR_ARG;
    }
}

bool needs_stream(int64_t op)
{
    return (op >= OP_EXTRACT_WORD && op <= OP_UNGET) || op == OP_CLEAR;
}
} // namespace

// mode 0: SmDatabaseIOStream(file_type, istringstream(data))
// mode 1: SmDatabaseIOFile + OpenFileForRead(path, file_type) (status -> *open_status)
// mode 2: SmDatabaseIO() (no stream)
// mode 3: SmDatabaseIO(file_type) (no stream)
// word_cap: longest word an extraction can produce (file / data length).
// ops[2*n_ops]: (op, arg) pairs. records[5*n_ops]: [status, rdstate, position, n_ints, n_dbls].
SC_EXPORT int32_t sc_topology_database_io_run(int32_t mode, const uint8_t* data, int64_t data_len,
                                              const char* path, int32_t file_type, int64_t word_cap,
                                              const int64_t* ops, int64_t n_ops,
                                              int64_t* open_status, int64_t* records,
                                              int64_t* ints, int64_t ints_cap,
                                              int64_t* ints_len, double* dbls,
                                              int64_t dbls_cap, int64_t* dbls_len)
{
    if (mode < 0 || mode > 3 || data_len < 0 || word_cap < 0 || n_ops < 0 || file_type < 0 || file_type > 3)
        return SC_ERR_ARG;
    if (mode == 1 && path == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmFileType ft = (SmFileType)file_type;
    std::istringstream iss(std::string((const char*)data, (size_t)data_len));
    std::unique_ptr<SmDatabaseIO> db;
    *open_status = 0;
    switch (mode)
    {
    case 0:
        db.reset(new SmDatabaseIOStream(ft, &iss));
        break;
    case 1:
    {
        SmDatabaseIOFile* f = new SmDatabaseIOFile();
        db.reset(f);
        SmStatus ost = f->OpenFileForRead(path, ft);
        *open_status = ost == SM_SUCCESS ? 0 : ost;
        break;
    }
    case 2:
        db.reset(new SmDatabaseIO());
        break;
    default:
        db.reset(new SmDatabaseIO(ft));
        break;
    }
    Out o;
    for (int64_t k = 0; k < n_ops; ++k)
    {
        int64_t op = ops[2 * k], arg = ops[2 * k + 1];
        bool ascii_type = op == OP_READ_TYPE && db->GetFileType() == SM_ASCII;
        if ((needs_stream(op) || ascii_type) && db->GetInStreamPtr() == nullptr)
            return SC_ERR_ARG;
        size_t i0 = o.ints.size(), d0 = o.dbls.size();
        int64_t st = run_op(*db, op, arg, (size_t)word_cap, o);
        if (st == SC_ERR_ARG)
            return SC_ERR_ARG;
        records[5 * k] = st == SM_SUCCESS ? 0 : st;
        records[5 * k + 1] = state_of(*db);
        records[5 * k + 2] = position_of(*db);
        records[5 * k + 3] = (int64_t)(o.ints.size() - i0);
        records[5 * k + 4] = (int64_t)(o.dbls.size() - d0);
    }
    *ints_len = (int64_t)o.ints.size();
    *dbls_len = (int64_t)o.dbls.size();
    if (*ints_len > ints_cap || *dbls_len > dbls_cap)
        return SC_ERR_BUFFER;
    for (size_t i = 0; i < o.ints.size(); ++i)
        ints[i] = o.ints[i];
    for (size_t i = 0; i < o.dbls.size(); ++i)
        dbls[i] = o.dbls[i];
    return SC_OK;
    SC_CATCH
}

#include "sc_shim.h"

extern "C" int sc_cpp_stdlib_is_libstdcxx(void) {
#ifdef __GLIBCXX__
    return 1;
#else
    return 0;
#endif
}
