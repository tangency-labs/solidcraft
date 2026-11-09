import math
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from matplotlib.patches import Rectangle, Polygon

BLUE = '#1f6fb2'; PALE = '#dce9f5'; GREY = '#999999'; DARK = '#555555'; FACE = '#f0f0f0'
plt.rcParams.update({'font.family': 'sans-serif',
                     'font.sans-serif': ['Helvetica', 'Arial', 'DejaVu Sans'],
                     'font.size': 14, 'svg.fonttype': 'none'})
fig, (a1, a2) = plt.subplots(1, 2, figsize=(14, 8.4))
for ax in (a1, a2):
    ax.set_aspect('equal')
    ax.axis('off')

# Old test: the issue's own case.
a1.set_title('Old test: centre offset\nagainst the radii', fontsize=20, fontweight='bold')
a1.annotate('', xy=(0, 11.6), xytext=(0, -1.2),
            arrowprops=dict(arrowstyle='-|>', color=GREY, lw=1.5, ls='--'))
a1.text(0.35, 11.3, 'direction V', fontsize=12)
a1.plot([0], [0], 'o', color='black', ms=9)
a1.text(0.45, -0.35, 'query point A\n(radius 0)', fontsize=12)
a1.plot([0], [10], 'o', color='black', ms=9)
a1.text(0.45, 9.75, 'vertex B (radius 0)', fontsize=12)
a1.annotate('', xy=(0, 9.75), xytext=(0, 0.25),
            arrowprops=dict(arrowstyle='-|>', color=BLUE, lw=3))
a1.text(-4.3, 5.6, 'directed answer\nB - A = 10, along V:\nadmissible', fontsize=12)
a1.plot([-0.9, -0.15], [5.75, 5.6], color=DARK, lw=0.8)
a1.text(1.3, 4.6, 'centre offset 10\n> radii 0 + 0\n=> culled', fontsize=14, fontweight='bold')
a1.text(-4.4, -3.6, 'View along the normal N. The old test\nculls this pair, so the query returns\nno answer.', fontsize=12)
a1.set_xlim(-4.5, 6.5)
a1.set_ylim(-4, 12.5)

# New test: the difference box against the wedge around +V and -V.
a2.set_title('New test: the difference box\nagainst the direction wedge', fontsize=20, fontweight='bold')
ang = math.radians(6)  # drawn wider than the 2 degrees it stands for
R = 11
for sgn in (1, -1):
    a2.add_patch(Polygon([(0, 0), (R * math.sin(ang), sgn * R * math.cos(ang)),
                          (-R * math.sin(ang), sgn * R * math.cos(ang))],
                         closed=True, facecolor=PALE, edgecolor=GREY, lw=1.5, ls='--'))
a2.text(-9.4, -8.4, 'differences within\n2 degrees of +V or -V\n(drawn wider)', fontsize=12)
a2.plot([-3.2, -0.72], [-7.5, -7.4], color=DARK, lw=0.8)
a2.plot([0], [0], 'o', color=DARK, ms=7)
a2.text(0.3, -0.7, 'zero difference', fontsize=12)
a2.add_patch(Rectangle((-1.2, 5.5), 2.6, 3.0, facecolor=FACE, edgecolor=BLUE, lw=3))
a2.text(1.7, 6.0, 'difference box B - A\nmeets the wedge => kept;\nits values |N x d x N|\nlie in [5.5, 8.6]', fontsize=12)
a2.add_patch(Rectangle((3.0, -4.0), 2.4, 2.2, facecolor=FACE, edgecolor=DARK, lw=1.5))
a2.text(3.0, -5.2, 'misses the wedge => culled', fontsize=12)
a2.text(-9.4, -15.2, 'Every bound uses outward-rounded intervals. A kept\npair is also culled when all its values are worse than\nthe best answer by more than the query tolerance.', fontsize=12)
a2.set_xlim(-9.5, 9.5)
a2.set_ylim(-15.5, 12.5)

fig.tight_layout()
fig.savefig('directed-culling-13960.svg')
fig.savefig('directed-culling-13960.png', dpi=100)
