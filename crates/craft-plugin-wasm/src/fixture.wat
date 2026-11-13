;; Original test component, encoded from the command-world WIT with wasm-tools 1.254.0.
(component
  (type $ty-__PACKAGE__/types@__VERSION__ (;0;)
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
  (import "__PACKAGE__/types@__VERSION__" (instance $__PACKAGE__/types@__VERSION__ (;0;) (type $ty-__PACKAGE__/types@__VERSION__)))
  (alias export $__PACKAGE__/types@__VERSION__ "json" (type $json (;1;)))
  (alias export $__PACKAGE__/types@__VERSION__ "plugin-error" (type $plugin-error (;2;)))
  (type $ty-__PACKAGE__/commands@__VERSION__ (;3;)
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
  (import "__PACKAGE__/commands@__VERSION__" (instance $__PACKAGE__/commands@__VERSION__ (;1;) (type $ty-__PACKAGE__/commands@__VERSION__)))
  (core module $main (;0;)
    (type (;0;) (func (param i32 i32 i32 i32 i32)))
    (type (;1;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;2;) (func))
    (import "__PACKAGE__/commands@__VERSION__" "run" (func $run (;0;) (type 0)))
    (memory (;0;) 1)
    (table 1 funcref)
    (global $heap (;0;) (mut i32) i32.const 4096)
    (export "memory" (memory 0))
    (export "cabi_realloc" (func $realloc))
    (export "__PACKAGE__/entry@__VERSION__#execute" (func $execute))
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
    (func $box (;2;) (type 2)
      i32.const 512
      i32.const 9
      i32.const 1024
      i32.const 33
      i32.const 0
      call $run
    )
    (func $execute (;3;) (type 1) (param i32 i32 i32 i32) (result i32)
      call $box
      call $box
      i32.const 0
    )
    (data (;0;) (i32.const 512) "app.write")
    (data (;1;) (i32.const 1024) "{\22length\22:2,\22width\22:3,\22height\22:4}")
    (data (;2;) (i32.const 2048) "app.blocked")
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
  (alias export $__PACKAGE__/types@__VERSION__ "json" (type $"#type4 json" (@name "json") (;4;)))
  (alias export $__PACKAGE__/types@__VERSION__ "plugin-error" (type $"#type5 plugin-error" (@name "plugin-error") (;5;)))
  (alias core export $wit-component-shim-instance "0" (core func $indirect-__PACKAGE__/commands@__VERSION__-run (;0;)))
  (core instance $__PACKAGE__/commands@__VERSION__ (;1;)
    (export "run" (func $indirect-__PACKAGE__/commands@__VERSION__-run))
  )
  (core instance $main (;2;) (instantiate $main
      (with "__PACKAGE__/commands@__VERSION__" (instance $__PACKAGE__/commands@__VERSION__))
    )
  )
  (alias core export $main "memory" (core memory $memory (;0;)))
  (alias core export $wit-component-shim-instance "$imports" (core table $"shim table" (;0;)))
  (alias export $__PACKAGE__/commands@__VERSION__ "run" (func $run (;0;)))
  (alias core export $main "cabi_realloc" (core func $realloc (;1;)))
  (core func $"#core-func2 indirect-__PACKAGE__/commands@__VERSION__-run" (@name "indirect-__PACKAGE__/commands@__VERSION__-run") (;2;) (canon lower (func $run) (memory $memory) (realloc $realloc) string-encoding=utf8))
  (core instance $fixup-args (;3;)
    (export "$imports" (table $"shim table"))
    (export "0" (func $"#core-func2 indirect-__PACKAGE__/commands@__VERSION__-run"))
  )
  (core instance $fixup (;4;) (instantiate $wit-component-fixup
      (with "" (instance $fixup-args))
    )
  )
  (type (;6;) (result $"#type4 json" (error $"#type5 plugin-error")))
  (type (;7;) (func (param "entry" string) (param "params" $"#type4 json") (result 6)))
  (alias core export $main "__PACKAGE__/entry@__VERSION__#execute" (core func $__PACKAGE__/entry@__VERSION__#execute (;3;)))
  (func $execute (;1;) (type 7) (canon lift (core func $__PACKAGE__/entry@__VERSION__#execute) (memory $memory) (realloc $realloc) string-encoding=utf8))
  (instance $command
    (export "json" (type $json))
    (export "plugin-error" (type $plugin-error))
    (export "execute" (func $execute)))
  (export "__PACKAGE__/entry@__VERSION__" (instance $command))
)
