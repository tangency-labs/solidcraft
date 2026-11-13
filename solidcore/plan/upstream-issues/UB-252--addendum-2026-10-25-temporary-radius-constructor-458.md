Updates: UB-252

PR: https://github.com/tangency-labs/solidcore/pull/644

Main's UB-252 protections and #458 closure remain unchanged. This follow-up is
limited to the generic temporary-radius constructor and scope wrapper:
`Kernel::fixes.temporary_radius_setup_errors` is default-off, supplied through
additive `new_with_fixes` and `with_temporary_radius_change_with_fixes` APIs.
Existing call shapes retain pinned ordinary-status handling. The runner regression
checks both policies; the offset-law regression proves failed evaluation leaves
both offset distances unchanged. `DirectTemporaryRadiusChange` remains outside
this follow-up's scope. No new native UB run or broader fillet completion is claimed.
