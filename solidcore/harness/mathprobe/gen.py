# Deterministic inputs per function, weighted toward what a geometry kernel feeds them.
import math, os, random, struct, sys
os.makedirs("inputs", exist_ok=True)
random.seed(20261017)
N = 200_000
TWO_PI = 2 * math.pi
def mix(*parts):  # parts: (weight, sampler)
    out = []
    for w, f in parts:
        out += [f() for _ in range(int(N * w))]
    return out
U = random.uniform
def near(c, s):  # around c within +-s
    return lambda: c + U(-s, s)
angle = [(0.5, lambda: U(-TWO_PI, TWO_PI)), (0.15, lambda: U(-4 * TWO_PI, 4 * TWO_PI)),
         (0.1, lambda: random.choice([-1, 1]) * random.randint(0, 8) * math.pi / 2 + U(-1e-6, 1e-6)),
         (0.1, lambda: U(-1e-3, 1e-3)), (0.1, lambda: U(-1e6, 1e6)), (0.05, lambda: U(-1e15, 1e15))]
logu = lambda lo, hi: (lambda: 10 ** U(lo, hi))
sets = {
    "sin": mix(*angle), "cos": mix(*angle), "tan": mix(*angle),
    "asin": mix((0.8, lambda: U(-1, 1)), (0.2, lambda: random.choice([-1, 1]) * (1 - 10 ** U(-16, -1)))),
    "acos": mix((0.8, lambda: U(-1, 1)), (0.2, lambda: random.choice([-1, 1]) * (1 - 10 ** U(-16, -1)))),
    "atan": mix((0.6, lambda: random.gauss(0, 3)), (0.4, lambda: random.choice([-1, 1]) * 10 ** U(-12, 12))),
    "sinh": mix((0.8, lambda: U(-20, 20)), (0.2, lambda: U(-700, 700))),
    "cosh": mix((0.8, lambda: U(-20, 20)), (0.2, lambda: U(-700, 700))),
    "ln": mix((0.7, logu(-12, 12)), (0.3, near(1, 1e-3))),
    "log10": mix((0.7, logu(-12, 12)), (0.3, near(1, 1e-3))),
    "cbrt": mix((0.7, lambda: U(-1e3, 1e3)), (0.3, lambda: random.choice([-1, 1]) * 10 ** U(-30, 30))),
}
pairs = {
    "atan2": [(random.gauss(0, 1) * 10 ** U(-6, 6), random.gauss(0, 1) * 10 ** U(-6, 6)) for _ in range(N)],
    "powf": [(10 ** U(-6, 6), random.choice([U(-4, 4), 0.5, 2.0, 3.0, 1 / 3, -1.0, -0.5, 1.5])) for _ in range(N)],
    "powi": [(random.gauss(0, 1) * 10 ** U(-3, 3), float(random.randint(-4, 8))) for _ in range(N)],
}
for k, v in sets.items():
    open(f"inputs/{k}.bin", "wb").write(struct.pack(f"<{len(v)}d", *v))
for k, v in pairs.items():
    flat = [a for p in v for a in p]
    open(f"inputs/{k}.bin", "wb").write(struct.pack(f"<{len(flat)}d", *flat))
print({k: len(v) for k, v in {**sets, **pairs}.items()})
