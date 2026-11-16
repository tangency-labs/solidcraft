Updates: WR-6

#### WR-6 addendum (2026-10-31): opt-in empty extent interval addition

`Kernel::fixes.periodic_extent_add_empty_interval` enables setting `[from, to]`
directly on an empty `SmPeriodicExtent1d` rather than triggering the sentinel
evaluation `(m_dPeriod - |m_dMax - m_dMin|) < ScaledZero` which mistakenly marked
the extent as full period `[0, period]`. It is false by default, preserving pinned
C++ v1.0.1 bit-parity across all callers. When enabled, addition to an empty extent
stores the exact interval unless the added span itself covers the full period.
