#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
set -eu
exe=${1:?usage: run_setback_executive.sh /path/to/native/executive}
$exe 40 30 20 2 3 4 -1 1
for sb in 8 20 0 5; do
echo "== setback $sb ease 0"
$exe 60 40 20 5 5 5 "$sb" 1
done
echo "== setback 8 ease 2"
$exe 60 40 20 5 5 5 8 1 2
for sb in 4.25 4.5 4.6 4.7175; do
echo "== short $sb"
$exe 20 20 20 2 3 4 "$sb"
done
echo "== shrink"
$exe 20 20 5.5 2 3 4 -1
echo "== concave"
$exe 20 20 20 1 1.5 2 -1 2
echo "== unequal dependency box"
$exe 20 20 20 2 2 3 -1 3
