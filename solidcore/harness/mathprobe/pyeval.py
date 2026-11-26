# Same functions through Python's math module = the platform C libm (glibc in Docker).
import math, os, struct, sys
out = sys.argv[1]; os.makedirs(out, exist_ok=True)
def safe(f):
    def g(*a):
        try: return f(*a)
        except (OverflowError, ValueError): return float("nan")  # flagged separately
    return g
fns = {"sin": math.sin, "cos": math.cos, "tan": math.tan, "asin": math.asin, "acos": math.acos,
       "atan": math.atan, "sinh": math.sinh, "cosh": math.cosh, "ln": math.log, "log10": math.log10,
       "cbrt": math.cbrt, "atan2": math.atan2, "powf": math.pow}
for name, f in fns.items():
    b = open(f"inputs/{name}.bin", "rb").read()
    v = struct.unpack(f"<{len(b)//8}d", b)
    ar = 2 if name in ("atan2", "powf") else 1
    r = [safe(f)(*v[i:i+ar]) for i in range(0, len(v), ar)]
    open(f"{out}/{name}.bin", "wb").write(struct.pack(f"<{len(r)}d", *r))
import platform; print(out, platform.platform())
