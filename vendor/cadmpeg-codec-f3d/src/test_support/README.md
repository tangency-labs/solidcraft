# F3D regression fixtures

The checked-in JSON and binary fixtures are synthetic. Their identifiers, text,
geometry and reference graphs are assembled from the documented field grammar.
Run `python3 generate_fixtures.py` in this directory to regenerate them.

Set `CADMPEG_F3D_VERIFICATION_FIXTURES` to a local fixture directory to run the
same assertions against the private verification inputs. The directory uses the
same relative paths and JSON shapes. Missing inputs fail the run. The private
set retains its own pinned case counts.

Keep original corpus carriers, corpus filenames and source paths outside Git.
