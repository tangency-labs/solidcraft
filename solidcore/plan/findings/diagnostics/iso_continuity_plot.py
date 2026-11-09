"""Plot exact native affine pieces with observed and expected continuity counts."""
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
base=Path(__file__).parent
p=base/'iso_continuity_native_plot.json'
r=json.loads(p.read_text(encoding='utf-8-sig'))
p.write_text(json.dumps(r)+'\n',encoding='utf-8')
after=base/'iso_continuity_native_plot_green.json'
expected=not after.exists()
if not expected:
 green=json.loads(after.read_text(encoding='utf-8-sig'))
 assert green['native_points']==r['native_points'] and green['observed_continuity_count']==3
pts=r['native_points']
fig,axes=plt.subplots(1,2,figsize=(8,5),sharex=True,sharey=True,constrained_layout=True)
for ax,title in zip(axes,['Before: 2 records for 3 knots',('Expected' if expected else 'After')+': 3 corresponding records']):
 ax.plot([p[1] for p in pts],[p[2] for p in pts],color='#1f6fb2',lw=2)
 ax.set(title=title,xlabel='native X');ax.grid(color='#ddd',lw=.5);ax.set_aspect('equal',adjustable='box')
axes[0].set_ylabel('native Y (projection along Z)')
axes[1].scatter([p[1] for p in pts],[p[2] for p in pts],c='black',s=25,zorder=3)
for p in pts:axes[1].annotate('t='+str(p[0]),(p[1],p[2]),xytext=(5,5),textcoords='offset points',fontsize=9)
fig.suptitle('Native isocurve: same geometry and view; varying-direction continuity metadata',fontsize=12)
fig.savefig(base/'iso_continuity_native_plot.png',dpi=170)
