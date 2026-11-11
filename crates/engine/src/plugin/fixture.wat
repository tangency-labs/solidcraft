;; Original test component, encoded from the command-world WIT with wasm-tools 1.254.0.
(component
  (type $ty-solidcraft:plugin/types@0.1.0 (;0;)
    (instance
      (type (;0;) string)
      (export (;1;) "json" (type (eq 0)))
      (type (;2;) (variant (case "failed" string) (case "denied" string) (case "limit" string) (case "invalid" string)))
      (export (;3;) "plugin-error" (type (eq 2)))
    )
  )
  (import "solidcraft:plugin/types@0.1.0" (instance $solidcraft:plugin/types@0.1.0 (;0;) (type $ty-solidcraft:plugin/types@0.1.0)))
  (alias export $solidcraft:plugin/types@0.1.0 "json" (type $json (;1;)))
  (alias export $solidcraft:plugin/types@0.1.0 "plugin-error" (type $plugin-error (;2;)))
  (type $ty-solidcraft:plugin/engine@0.1.0 (;3;)
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
  (import "solidcraft:plugin/engine@0.1.0" (instance $solidcraft:plugin/engine@0.1.0 (;1;) (type $ty-solidcraft:plugin/engine@0.1.0)))
  (core module $main (;0;)
    (type (;0;) (func (param i32 i32 i32 i32 i32)))
    (type (;1;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;2;) (func))
    (import "solidcraft:plugin/engine@0.1.0" "run" (func $run (;0;) (type 0)))
    (memory (;0;) 1)
    (table 1 funcref)
    (global $heap (;0;) (mut i32) i32.const 4096)
    (export "memory" (memory 0))
    (export "cabi_realloc" (func $realloc))
    (export "solidcraft:plugin/command@0.1.0#execute" (func $execute))
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
    (data (;0;) (i32.const 512) "solid.box")
    (data (;1;) (i32.const 1024) "{\22length\22:2,\22width\22:3,\22height\22:4}")
    (data (;2;) (i32.const 2048) "file.export")
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
  (alias export $solidcraft:plugin/types@0.1.0 "json" (type $"#type4 json" (@name "json") (;4;)))
  (alias export $solidcraft:plugin/types@0.1.0 "plugin-error" (type $"#type5 plugin-error" (@name "plugin-error") (;5;)))
  (alias core export $wit-component-shim-instance "0" (core func $indirect-solidcraft:plugin/engine@0.1.0-run (;0;)))
  (core instance $solidcraft:plugin/engine@0.1.0 (;1;)
    (export "run" (func $indirect-solidcraft:plugin/engine@0.1.0-run))
  )
  (core instance $main (;2;) (instantiate $main
      (with "solidcraft:plugin/engine@0.1.0" (instance $solidcraft:plugin/engine@0.1.0))
    )
  )
  (alias core export $main "memory" (core memory $memory (;0;)))
  (alias core export $wit-component-shim-instance "$imports" (core table $"shim table" (;0;)))
  (alias export $solidcraft:plugin/engine@0.1.0 "run" (func $run (;0;)))
  (alias core export $main "cabi_realloc" (core func $realloc (;1;)))
  (core func $"#core-func2 indirect-solidcraft:plugin/engine@0.1.0-run" (@name "indirect-solidcraft:plugin/engine@0.1.0-run") (;2;) (canon lower (func $run) (memory $memory) (realloc $realloc) string-encoding=utf8))
  (core instance $fixup-args (;3;)
    (export "$imports" (table $"shim table"))
    (export "0" (func $"#core-func2 indirect-solidcraft:plugin/engine@0.1.0-run"))
  )
  (core instance $fixup (;4;) (instantiate $wit-component-fixup
      (with "" (instance $fixup-args))
    )
  )
  (type (;6;) (result $"#type4 json" (error $"#type5 plugin-error")))
  (type (;7;) (func (param "entry" string) (param "params" $"#type4 json") (result 6)))
  (alias core export $main "solidcraft:plugin/command@0.1.0#execute" (core func $solidcraft:plugin/command@0.1.0#execute (;3;)))
  (func $execute (;1;) (type 7) (canon lift (core func $solidcraft:plugin/command@0.1.0#execute) (memory $memory) (realloc $realloc) string-encoding=utf8))
  (component $solidcraft:plugin/command@0.1.0-shim-component (;0;)
    (type (;0;) string)
    (import "import-type-json" (type (;1;) (eq 0)))
    (type (;2;) (variant (case "failed" string) (case "denied" string) (case "limit" string) (case "invalid" string)))
    (import "import-type-plugin-error" (type (;3;) (eq 2)))
    (import "import-type-json0" (type (;4;) (eq 1)))
    (import "import-type-plugin-error0" (type (;5;) (eq 3)))
    (type (;6;) (result 4 (error 5)))
    (type (;7;) (func (param "entry" string) (param "params" 4) (result 6)))
    (import "import-func-execute" (func (;0;) (type 7)))
    (export (;8;) "json" (type 1))
    (export (;9;) "plugin-error" (type 3))
    (type (;10;) (result 8 (error 9)))
    (type (;11;) (func (param "entry" string) (param "params" 8) (result 10)))
    (export (;1;) "execute" (func 0) (func (type 11)))
  )
  (instance $solidcraft:plugin/command@0.1.0-shim-instance (;2;) (instantiate $solidcraft:plugin/command@0.1.0-shim-component
      (with "import-func-execute" (func $execute))
      (with "import-type-json" (type $"#type4 json"))
      (with "import-type-plugin-error" (type $"#type5 plugin-error"))
      (with "import-type-json0" (type $"#type4 json"))
      (with "import-type-plugin-error0" (type $"#type5 plugin-error"))
    )
  )
  (export $solidcraft:plugin/command@0.1.0 (;3;) "solidcraft:plugin/command@0.1.0" (instance $solidcraft:plugin/command@0.1.0-shim-instance))
  (@producers
    (processed-by "wit-component" "0.254.0")
  )
)
