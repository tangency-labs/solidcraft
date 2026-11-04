# SPDX-License-Identifier: Apache-2.0
"""Build regression fixtures from format fields and invented model values."""

from pathlib import Path
import json,struct,uuid
ROOT=Path(__file__).resolve().parent
def u(n):return struct.pack('<I',n)
def q(n):return struct.pack('<Q',n)
def d(n):return struct.pack('<d',n)
def a(s):return u(len(s))+s.encode('ascii')
def w(s):return u(len(s.encode('utf-16le'))//2)+s.encode('utf-16le')
def ref(n):return b'\0' if n is None else b'\1'+q(n)+bytes(2)
def head(tag,n):return a(tag)+u(n)
def guid(n):return str(uuid.UUID(int=n))
def ident(x=0,y=0,z=0):return [1.,0.,0.,float(x),0.,1.,0.,float(y),0.,0.,1.,float(z),0.,0.,0.,1.]
def save(name,data):
 p=ROOT/name;p.parent.mkdir(parents=True,exist_ok=True)
 if isinstance(data,bytes):p.write_bytes(data)
 else:p.write_text(json.dumps(data,indent=2)+'\n')

# Indexed placement construction fields, assembled in specification order.
for index,(name,v,marked,identity,align) in enumerate([
 ('extra-scalar-v14',14,True,False,[0.,0.,0.]),('unmarked-v4',4,False,False,[0.,0.,0.]),('explicit-v4',4,True,False,[0.,0.,0.]),('identity-v6',6,True,True,[0.,0.,0.]),('unmarked-alignment-v4',4,False,False,[0.,1.,0.])]):
 raw=bytearray(head('300',10+index)+bytes(10));fields={k:None for k in ['linear_offset_on_periodic_face','align_up_direction','arc_length_for_periodic_face','axis_flipped','use_u_direction','definer_creator','extra_offset','global_parameters','inferred_x_axis','inferred_z_axis','z_flip','identity','matrix','old_rotation','pick_point_parameter','pick_point','edge_references','origin','plane_references','x_axis','z_axis','ref_type','resize_type','values']}
 def scalar(key,value,encoded):fields[key]=dict(value=value,offset=17+len(raw));raw.extend(encoded)
 def reference(key,target):
  encoded=ref(target);fields[key]=dict(offset=17+len(raw),encoding=encoded.hex());raw.extend(encoded)
 def refs(key,targets):
  count=dict(value=len(targets),offset=17+len(raw));raw.extend(u(len(targets)));members=[]
  for target in targets:
   encoded=ref(target);members.append(dict(offset=17+len(raw),encoding=encoded.hex()));raw.extend(encoded)
  fields[key]=dict(count=count,members=members)
 if v>=14:scalar('linear_offset_on_periodic_face',0,b'\0')
 scalar('align_up_direction',align,b''.join(map(d,align)))
 if v>=11:scalar('arc_length_for_periodic_face',0.,d(0.))
 if v>=8:scalar('axis_flipped',0,b'\0')
 if v>=10:scalar('use_u_direction',1,b'\1')
 if v>=6:reference('definer_creator',None)
 if v>=13:reference('extra_offset',100)
 if v>=9:
  vals=[]
  for _ in range(4):vals.append(dict(offset=17+len(raw),encoding='00'));raw.extend(b'\0')
  fields['global_parameters']=vals
 scalar('inferred_x_axis',255 if align[1] else 0,bytes([255 if align[1] else 0]));scalar('inferred_z_axis',7 if v==6 else 0,bytes([7 if v==6 else 0]));scalar('z_flip',0,b'\0')
 if marked:scalar('identity',identity,bytes([identity]))
 if not identity:
  m=ident(2,3,4);scalar('matrix',[m[i:i+4] for i in range(0,16,4)],b''.join(map(d,m)))
 if v>=7:scalar('old_rotation',0.5,d(.5))
 scalar('pick_point',[0.,0.],bytes(16))
 if v>=5:refs('edge_references',[])
 reference('origin',None);refs('plane_references',[]);reference('x_axis',None);reference('z_axis',None);scalar('ref_type',3 if v==14 else 2,u(3 if v==14 else 2))
 if v>=12:scalar('resize_type',0,b'\0')
 refs('values',[200] if v==14 else []);base=dict(offset=17+len(raw),value='aa55');raw.extend(bytes.fromhex('aa55'))
 native=dict(id='f3d:Design/BulkStream.dat:design-placement-construction#17',record_index=10+index,class_tag='300',type_version=v,byte_offset=17,member_offset=38,paired_byte_offset=17+len(raw),fields=fields,base_level=base)
 save('placement_construction/'+name+'.json',dict(source_bytes=raw.hex(),native=native))

for i,name in enumerate(['translation','rotation','identity','offset-three','offset-four']):
 matrix=ident(i+1,-i,2*i)
 if i==1:matrix=[0.,-1.,0.,2.,1.,0.,0.,3.,0.,0.,1.,4.,0.,0.,0.,1.]
 if i==2:matrix=ident()
 n=10+i;following=1000+10*i;tag=str(300+i);next_tag=str(400+i)
 raw=head(tag,n)+bytes(11)+b''.join(map(d,matrix))+b'\1\0'+head(next_tag,following)
 native=dict(record_index=n,byte_offset=17,class_tag=tag,transform=[matrix[j:j+4] for j in range(0,16,4)],transform_offset=39,following_record_index=following,following_byte_offset=169,following_class_tag=next_tag)
 save('construction_transforms/'+name+'.json',dict(source_bytes=raw.hex(),native=native))

# Attributed Component carriers: all source-specific identifiers are invented.
def attributed(n,component,ordinal,discriminator,segment,matrix=None,name=''):
 raw=a('373')+q(n)+a(name)+b'\1'+u(1)+ref(component)+u(discriminator)+b'\1'+u(3)
 for key,kind,value in [('GatedByParent','IntrinsicMetaTypebool',b'\0'),('ResolutionTime','IntrinsicMetaTypeHString',w('')),('SelfGated','IntrinsicMetaTypebool',b'\0')]:raw+=a(key)+a(kind)+value
 key=str(n);component_guid=guid(10000+component)
 raw+=bytes(2)+u(ordinal)+w(component_guid)+w(key)+b'\1'+q(500+n)+b'\0\1'+u(segment)+b'\0'+ref(component)
 raw+=b'\1' if matrix is None else b'\0'+b''.join(map(d,matrix))
 raw+=bytes(9)+ref(6)
 return raw+head('262',n+1),dict(label='synthetic-attributed-'+str(n),source_hex=(raw+head('262',n+1)).hex(),component=component,ordinal=ordinal,path_discriminator=discriminator,component_guid=component_guid,occurrence_key=key,key_record_index=500+n,key_segment_index=segment,owner=6,matrix=matrix)
rows=[]
for i,(ordinal,disc,seg,mat) in enumerate([(1,0,1,None),(2,0,10,None),(3,8,69,None),(2,1,1,ident(2,3,4)),(4,7,10,ident()),(1,8,69,ident(-2,4,6))]):
 raw,row=attributed(20+i,187,ordinal,disc,seg,mat);rows.append(row)
save('xref/attributed_records.json',rows);save('xref/attributed-occurrence.bin',attributed(40,187,2,7,10,ident(2,3,4))[0]);save('xref/root-instance.bin',attributed(50,3,0,0,10,None,'rootInstance')[0])

COMPONENT='CE2913AA-CFE0-4F04-9102-24424ED3BCFA';COMMON='3B6814F4-D87B-46BD-AD9D-0BE07880F7BB';TARGET='E03784ED-5E19-4E14-B9F2-3B07017018CD'
def legacy(n,role,matrix):
 target=b'\1'+q(3)+b'\1'+q(10000+n)+b'\0'+a(TARGET)+b'\0'
 raw=a('380')+q(n)+a('')+b'\1'+u(1)+target+u(1)+u(1)+b'\0'+u(1)+w(guid(20000+n))+w(guid(30000+n))+b'\0'+target
 raw+=b'\1' if matrix is None else b'\0'+b''.join(map(d,matrix))
 return raw+u(0)+w(role)+bytes([0,1,4,0,0,0,0,0,0,0,0,0])
rows=[]
for i,matrix in enumerate([None,ident(2,3,4)]):
 role=guid(40000+i);rows.append(dict(label='synthetic-legacy-'+str(i),type_guid=COMPONENT,version=2,module='Component',class_tag='380',entity=10+i,source_hex=legacy(10+i,role,matrix).hex(),role=role,matrix=matrix))
raw=a('380')+q(40)+a('')+b'\1'+u(1)+ref(100)+u(0)+b'\0\1'+u(1)+ref(101)+ref(3)+ref(6)
rows.append(dict(label='synthetic-common-data',type_guid=COMMON,version=0,module='CommonData',class_tag='380',entity=40,source_hex=raw.hex(),role=None,matrix=None));save('xref/original_records.json',rows)

# Text bodies with independent style and orientation states.
rows=[]
presentation=bytes([0,0,0,0,1])+struct.pack('<5f',0.,0.,0.,1.,1.)+bytes(5)
for i,(style,weight,neutral) in enumerate([(0,400,True),(1,750,True),(2,400,False),(3,750,False),(0,400,False),(0,400,False)]):
 text='Synthetic '+str(i);font='Arial';legacy_style=i in [1,2,3];version=4 if legacy_style else 0
 raw=a('376')+q(20+i)+a('')
 raw+=(b'\1'+u(0)) if legacy_style else b'\0'
 raw+=b'\1'+u(2)
 for key,value in [('EntityGenesis',0),('txt_tag' if legacy_style else 'textex_tag',20+i)]:raw+=a(key)+a('IntrinsicMetaTypeuint64')+q(value)
 if legacy_style:
  raw+=d(0.)+bytes([style])+bytes(4)+struct.pack('<4f',0.,0.,0.,1.)+w(font)+d(1.)+bytes(2)+bytes(16)+bytes(11)+w(text)+u(0)
  members=bytearray(15);members[3:7]=struct.pack('<i',weight);raw+=members+presentation+ref(42)
 else:
  raw+=b'\1'+d(1.)+struct.pack('<4f',0.,0.,0.,1.)+w(font)+b'\0'+d(1.)+u(1)+bytes([int(i==4),0,0])+w(text)+u(1)+bytes([int(i==5)])+struct.pack('<i',weight)+bytes(5)
  matrix=ident();raw+=b''.join(map(d,matrix))+presentation+ref(42)
 rows.append(dict(label='synthetic-text-'+str(i),class_tag='376',class_version=version,record_index=20+i,source_hex=raw.hex(),text=text,font_family=font,font_weight=weight,height_cm=1.,neutral_state=neutral))
save('sketch_text/original_records.json',rows)

# Class-specific relation payloads and nullable parameter references.
rows=[]
def relation(payload,mask):return head('298',7)+bytes(8)+b'\1'+u(2)+ref(300)+u(1)+ref(301)+u(0)+b'\0'+payload+ref(201)+q(mask)+u(2)+ref(300)+ref(301)+b'\0'
def case(kind,version,payload,mask,flags=None,neutral=None):
 guids={'tangent':'24DB790E-3DCD-4336-AFA3-6F119EF2239B','circular':'8269E861-0BB7-47E0-9911-5AE3EC475058','rectangular':'40800FB9-C2BE-494E-A047-7D76E82B9F6C','text-frame':'8B369926-123F-4F9D-878E-6D4C076128D3'}
 rows.append(dict(label='synthetic-'+kind+'-'+str(len(rows)),type_guid=guids[kind],class_version=version,record_index=7,class_tag='298',source_hex=relation(payload,mask).hex(),rectangular_flags=flags,pattern_neutral_state=neutral))
case('tangent',0,bytes([0,1,0]),0x100)
for angle_ref,count_ref,suppressed in [(20,21,[]),(None,21,[]),(20,None,[1])]:
 payload=ref(angle_ref)+ref(count_ref)+d(3.141592653589793)+u(4)+u(0)+u(len(suppressed))+b''.join(map(u,suppressed))+b'\0';case('circular',3,payload,0x10000000,neutral=not suppressed)
for flags,parameter in [([0,0,0],20),([1,0,0],None),([0,1,0],20)]:
 payload=bytes(flags)+u(0)+bytes(8)
 for count,direction,distance in [(4,[1.,0.,0.],-2.),(1,[0.,1.,0.],0.)]:payload+=u(count)+ref(parameter)+b''.join(map(d,direction))+d(distance)+ref(21)
 case('rectangular',5,payload,0x20000000,flags,flags in ([0,0,0],[1,0,0]))
case('text-frame',0,ref(None)+ref(300),0x10000000000)
save('sketch_relations/original_records.json',rows)

# Synthetic fixed Hem class tails, independent of a CAD source archive.
for name,shift,flipped in [('flat',0,False),('flipped',4,True)]:
 common=85+shift;raw=bytearray(494+shift)
 def put(at,encoded):raw[at:at+len(encoded)]=encoded
 put(common,u(3)+u(1));put(common+8,ref(708));put(common+19,ref(724));put(common+30,u(1));put(common+36,u(4));raw[common+40]=flipped;put(common+42,ref(703));put(common+53,ref(706));put(common+71,d(.25));put(common+108,ref(717 if flipped else 710));put(common+135,ref(754 if flipped else 717));save('hem/'+name+'.bin',bytes(raw))

print('Synthetic F3D fixtures regenerated.')
