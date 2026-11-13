Updates: WR-91

#### WR-91 (update, 2026-10-26). Fixed in v1.0.1

Fixed in v1.0.1 (3fd5b95): `SmVertex::GetVertexuseOfFace` tests the vertex use's face against `pFace` again (`if ( pThisFace == pFace )`, `SmVertex.cpp:260`), so it returns only a vertex use of the target face.
