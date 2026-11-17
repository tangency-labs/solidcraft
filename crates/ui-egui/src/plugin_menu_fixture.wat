;; Original test component, derived from the engine's plugin fixture: its entry passes its
;; parameters straight to `solid.box` and returns that command's reply.
(component
  (type $ty-PLUGIN_PACKAGE/types@PLUGIN_VERSION (;0;)
    (instance
      (type (;0;) string)
      (export (;1;) "json" (type (eq 0)))
      (type $command-failure (record (field "command" string) (field "reason" string)))
      (export "command-failure" (type $command-failure-export (eq $command-failure)))
      (type $field-failure (record (field "field" string) (field "reason" string)))
      (export "field-failure" (type $field-failure-export (eq $field-failure)))
      (type $plugin-error (variant (case "permission-denied" string) (case "command-failed" $command-failure-export) (case "limit-exceeded" string) (case "invalid-manifest" $field-failure-export) (case "invalid-input" $field-failure-export) (case "trap" string) (case "cancelled")))
      (export "plugin-error" (type (eq $plugin-error)))
    )
  )
  (import "PLUGIN_PACKAGE/types@PLUGIN_VERSION" (instance $PLUGIN_PACKAGE/types@PLUGIN_VERSION (;0;) (type $ty-PLUGIN_PACKAGE/types@PLUGIN_VERSION)))
  (alias export $PLUGIN_PACKAGE/types@PLUGIN_VERSION "json" (type $json (;1;)))
  (alias export $PLUGIN_PACKAGE/types@PLUGIN_VERSION "plugin-error" (type $plugin-error (;2;)))
  (type $ty-PLUGIN_PACKAGE/commands@PLUGIN_VERSION (;3;)
    (instance
      (alias outer 1 $json (type (;0;)))
      (export (;1;) "json" (type (eq 0)))
      (alias outer 1 $plugin-error (type (;2;)))
      (export (;3;) "plugin-error" (type (eq 2)))
      (type (;4;) (result 1 (error 3)))
      (type (;5;) (func (param "command" string) (param "args" 1) (result 4)))
      (export (;0;) "run" (func (type 5)))
    )
  )
  (import "PLUGIN_PACKAGE/commands@PLUGIN_VERSION" (instance $PLUGIN_PACKAGE/commands@PLUGIN_VERSION (;1;) (type $ty-PLUGIN_PACKAGE/commands@PLUGIN_VERSION)))
  (core module $main (;0;)
    (type (;0;) (func (param i32 i32 i32 i32 i32)))
    (type (;1;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;2;) (func))
    (import "PLUGIN_PACKAGE/commands@PLUGIN_VERSION" "run" (func $run (;0;) (type 0)))
    (memory (;0;) 1)
    (table 1 funcref)
    (global $heap (;0;) (mut i32) i32.const 4096)
    (export "memory" (memory 0))
    (export "cabi_realloc" (func $realloc))
    (export "PLUGIN_PACKAGE/entry@PLUGIN_VERSION#execute" (func $execute))
    (func $realloc (;1;) (type 1) (param i32 i32 i32 i32) (result i32)
      (local $at i32)
      global.get $heap
      local.tee $at
      local.get 3
      i32.add
      i32.const 7
      i32.add
      i32.const -8
      i32.and
      global.set $heap
      local.get $at
    )
    (func $execute (;2;) (type 1) (param i32 i32 i32 i32) (result i32)
      i32.const 512
      i32.const 9
      local.get 2
      local.get 3
      i32.const 0
      call $run
      i32.const 0
    )
    (data (;0;) (i32.const 512) "solid.box")
    (@producers
      (processed-by "wit-component" "0.254.0")
    )
  )
  (core module $wit-component-shim-module (;1;)
    (type (;0;) (func (param i32 i32 i32 i32 i32)))
    (table (;0;) 1 1 funcref)
    (export "0" (func 0))
    (export "$imports" (table 0))
    (func (;0;) (type 0) (param i32 i32 i32 i32 i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      local.get 4
      i32.const 0
      call_indirect (type 0)
    )
    (@producers
      (processed-by "wit-component" "0.254.0")
    )
  )
  (core module $wit-component-fixup (;2;)
    (type (;0;) (func (param i32 i32 i32 i32 i32)))
    (import "" "0" (func (;0;) (type 0)))
    (import "" "$imports" (table (;0;) 1 1 funcref))
    (elem (;0;) (i32.const 0) func 0)
    (@producers
      (processed-by "wit-component" "0.254.0")
    )
  )
  (core instance $wit-component-shim-instance (;0;) (instantiate $wit-component-shim-module))
  (alias export $PLUGIN_PACKAGE/types@PLUGIN_VERSION "json" (type $"#type4 json" (@name "json") (;4;)))
  (alias export $PLUGIN_PACKAGE/types@PLUGIN_VERSION "plugin-error" (type $"#type5 plugin-error" (@name "plugin-error") (;5;)))
  (alias core export $wit-component-shim-instance "0" (core func $indirect-PLUGIN_PACKAGE/commands@PLUGIN_VERSION-run (;0;)))
  (core instance $PLUGIN_PACKAGE/commands@PLUGIN_VERSION (;1;)
    (export "run" (func $indirect-PLUGIN_PACKAGE/commands@PLUGIN_VERSION-run))
  )
  (core instance $main (;2;) (instantiate $main
      (with "PLUGIN_PACKAGE/commands@PLUGIN_VERSION" (instance $PLUGIN_PACKAGE/commands@PLUGIN_VERSION))
    )
  )
  (alias core export $main "memory" (core memory $memory (;0;)))
  (alias core export $wit-component-shim-instance "$imports" (core table $"shim table" (;0;)))
  (alias export $PLUGIN_PACKAGE/commands@PLUGIN_VERSION "run" (func $run (;0;)))
  (alias core export $main "cabi_realloc" (core func $realloc (;1;)))
  (core func $"#core-func2 indirect-PLUGIN_PACKAGE/commands@PLUGIN_VERSION-run" (@name "indirect-PLUGIN_PACKAGE/commands@PLUGIN_VERSION-run") (;2;) (canon lower (func $run) (memory $memory) (realloc $realloc) string-encoding=utf8))
  (core instance $fixup-args (;3;)
    (export "$imports" (table $"shim table"))
    (export "0" (func $"#core-func2 indirect-PLUGIN_PACKAGE/commands@PLUGIN_VERSION-run"))
  )
  (core instance $fixup (;4;) (instantiate $wit-component-fixup
      (with "" (instance $fixup-args))
    )
  )
  (type (;6;) (result $"#type4 json" (error $"#type5 plugin-error")))
  (type (;7;) (func (param "entry" string) (param "params" $"#type4 json") (result 6)))
  (alias core export $main "PLUGIN_PACKAGE/entry@PLUGIN_VERSION#execute" (core func $PLUGIN_PACKAGE/entry@PLUGIN_VERSION#execute (;3;)))
  (func $execute (;1;) (type 7) (canon lift (core func $PLUGIN_PACKAGE/entry@PLUGIN_VERSION#execute) (memory $memory) (realloc $realloc) string-encoding=utf8))
  (instance $command
    (export "json" (type $json))
    (export "plugin-error" (type $plugin-error))
    (export "execute" (func $execute)))
  (export "PLUGIN_PACKAGE/entry@PLUGIN_VERSION" (instance $command))
)
