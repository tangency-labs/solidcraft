"""Exact binary64-input monotonicity proof for the rational gen1839 witness.

This is an independent diagnostic oracle, not a fixture-specific library path.
Only Python's standard library is needed. Controls are already homogeneous.
"""

import json
import math
from fractions import Fraction
from pathlib import Path


def polynomial(controls):
    a, b, c = map(Fraction, controls)
    return [a, 2 * (b - a), a - 2 * b + c]


def product(a, b):
    result = [Fraction(0)] * (len(a) + len(b) - 1)
    for i, left in enumerate(a):
        for j, right in enumerate(b):
            result[i + j] += left * right
    return result


def derivative_numerator(numerator, denominator):
    left = product([numerator[1], 2 * numerator[2]], denominator)
    right = product(numerator, [denominator[1], 2 * denominator[2]])
    result = [a - b for a, b in zip(left, right)]
    assert result[3] == 0
    return result[:3]


def evaluate(coefficients, parameter):
    return sum(c * parameter**i for i, c in enumerate(coefficients))


def bernstein_on(coefficients, low, high):
    c0, c1, c2 = coefficients
    h = high - low
    q0 = c0 + c1 * low + c2 * low * low
    q1 = h * (c1 + 2 * c2 * low)
    q2 = h * h * c2
    return [q0, q0 + q1 / 2, q0 + q1 + q2]


def main():
    cases = json.loads(Path(__file__).with_name("projected_pair_all_cases_3060.json").read_text())
    case = next(row["inputs"] for row in cases if row["name"] == "gen1839")
    assert case["c1"] == case["c2"]
    assert case["vecs"][0] == [0.0, 0.0, 1.0]
    definition = case["c1"]["bspline"]
    assert definition["p"] == 2 and definition["u"] == [0, 0, 0, 1, 1, 1]
    controls = definition["cpts"]
    assert all(point[3] > 0 for point in controls)
    polynomials = [polynomial([point[axis] for point in controls]) for axis in range(4)]
    ia, ib = [list(map(Fraction, case[key])) for key in ("ivl1", "ivl2")]
    low, high = min(ia[0], ib[0]), max(ia[1], ib[1])
    assert 0 <= low < high <= 1
    for axis, sign in [(0, -1), (1, 1)]:
        coefficients = bernstein_on(derivative_numerator(polynomials[axis], polynomials[3]), low, high)
        assert all(sign * coefficient > 0 for coefficient in coefficients)
        print("xy"[axis], "strict derivative sign", sign,
              "Bernstein coefficients", [float(c) for c in coefficients])

    def point(parameter):
        weight = evaluate(polynomials[3], parameter)
        return [float(evaluate(polynomials[axis], parameter) / weight) for axis in range(2)]

    corners = [(math.dist(point(s), point(t)), float(s), float(t)) for s in ia for t in ib]
    for distance, s, t in sorted(corners, reverse=True):
        print(f"corner ({s:.17g}, {t:.17g}): {distance:.17g}")


if __name__ == "__main__":
    main()
