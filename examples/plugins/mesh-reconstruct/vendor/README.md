# Guest-only dependency patch

`truck-geotrait` 0.4.0 is Apache-2.0, by Yoshinori Tanimura / RICOS Co. Ltd.
Source: https://github.com/ricosjp/truck/tree/487292c3686c4efd4b4c01acef9ff86136f6f630/truck-geotrait

The original source file hashes are recorded in UPSTREAM.json and the Apache
license is included. Local changes are marked `SolidCraft:`. The optional
`random-tests` feature retains the upstream randomized test helpers and their
rand/getrandom dependencies. They are excluded from the normal guest build;
production numerical traits and algorithms are unchanged. The upstream test
files remain and can run with that feature enabled.

This patch applies only to the standalone plugin manifest. It does not change
the host application's dependency resolution or replace its runtime.
