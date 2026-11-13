"""Independent official-reader check of an actual native GPU test output.
Run inside an isolated validation environment with the official OpenEXR and NumPy modules. No renderer dependencies,
source code, original images or user settings are modified.
"""
from pathlib import Path
import argparse,hashlib,json
import OpenEXR
import numpy as np

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('directory',type=Path)
args=parser.parse_args();root=args.directory.resolve()
expected=json.loads((root/'expected.json').read_text(encoding='utf-8'))
path=root/'raw-radiance.exr'
assert path.is_file() and path.stat().st_size<2*1024*1024
before=hashlib.sha256(path.read_bytes()).hexdigest()
with OpenEXR.File(str(path),separate_channels=True) as image:
 assert len(image.parts)==1
 channels=image.channels();header=image.header()
 assert set(channels)=={'R','G','B','A'}
 pixels={name:channel.pixels for name,channel in channels.items()}
 for value in pixels.values():
  assert value.dtype==np.float32 and value.shape==(expected['height'],expected['width'])
  assert np.isfinite(value).all()
 assert header['compression']==OpenEXR.NO_COMPRESSION
 assert header['lineOrder']==OpenEXR.INCREASING_Y
 assert tuple(header['dataWindow'][0])==(0,0)
 assert tuple(header['dataWindow'][1])==(expected['width']-1,expected['height']-1)
 assert tuple(header['displayWindow'][0])==tuple(header['dataWindow'][0])
 assert tuple(header['displayWindow'][1])==tuple(header['dataWindow'][1])
 assert all((pixels[c]>=0).all() for c in 'RGB')
 assert ((pixels['A']>=0)&(pixels['A']<=1)).all()
 assert np.count_nonzero(pixels['A']==0)>0
 assert all((pixels[c][pixels['A']==0]==0).all() for c in 'RGB')
 maximum=max(float(pixels[c].max()) for c in 'RGB')
 assert maximum>2 and np.float32(maximum)==np.float32(expected['maximum_raw_rgb'])
 chromaticities=np.asarray(header['chromaticities']).reshape(-1)
 assert np.allclose(chromaticities,[.64,.33,.30,.60,.15,.06,.3127,.3290],rtol=0,atol=1e-7)
 assert 'coverage-premultiplied' in header['comments']
 assert 'before exposure' in header['comments']
 report={'status':'PASS','reader':'official OpenEXR Python module','reader_version':OpenEXR.__version__,
         'numpy_version':np.__version__,'native_file_sha256':before,'shape':[expected['height'],expected['width']],
         'channels':sorted(channels),'sample_type':'FLOAT32','compression':'NONE','parts':1,
         'maximum_raw_rgb':maximum,'alpha_min':float(pixels['A'].min()),'alpha_max':float(pixels['A'].max()),
         'rgb_above_one_pixels':int(np.count_nonzero(np.maximum.reduce([pixels[c] for c in 'RGB'])>1)),
         'chromaticities':chromaticities.tolist(),
         'scope':'independent decode of actual GPU-produced master, header/precision/HDR range verification; not a transport convergence benchmark'}
assert hashlib.sha256(path.read_bytes()).hexdigest()==before
(root/'reference-reader.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
print(json.dumps(report,indent=2))
