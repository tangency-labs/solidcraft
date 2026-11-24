;; Original exporter test component, written by hand after the command-world fixture (wasm-tools
;; 1.254.0 encoding) for the exporter-plugin world. Its export slots by first letter:
;; c (copy) returns the reply of document.inspect as the file; e returns an error; t traps;
;; l loops forever; w asks for file.export, which an exporter is never granted.
(component
  (type $ty-__PACKAGE__/types@__VERSION__ (;0;)
    (instance
      (type (;0;) string)
      (export (;1;) "json" (type (eq 0)))
      (type $command-failure (record (field "command" string) (field "reason" string)))
      (export "command-failure" (type $command-failure-export (eq $command-failure)))
      (type $field-failure (record (field "field" string) (field "reason" string)))
      (export "field-failure" (type $field-failure-export (eq $field-failure)))
      (type $plugin-error (variant (case "permission-denied" string) (case "command-failed" $command-failure-export) (case "limit-exceeded" string) (case "invalid-manifest" $field-failure-export) (case "invalid-input" $field-failure-export) (case "trap" string) (case "cancelled") (case "unsupported" string)))
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
    (import "__PACKAGE__/commands@__VERSION__" "run" (func $run (;0;) (type 0)))
    (memory (;0;) 4)
    (global $heap (;0;) (mut i32) i32.const 8192)
    (export "memory" (memory 0))
    (export "cabi_realloc" (func $realloc))
    (export "__PACKAGE__/formats-provider@__VERSION__#export-file" (func $export))
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
    (func $export (;2;) (type 1) (param i32 i32 i32 i32) (result i32)
      (local $c i32)
      local.get 0
      i32.load8_u
      local.set $c
      local.get $c
      i32.const 116
      i32.eq
      if
        unreachable
      end
      local.get $c
      i32.const 108
      i32.eq
      if
        loop $forever
          br $forever
        end
      end
      local.get $c
      i32.const 101
      i32.eq
      if
        i32.const 64
        i32.const 1
        i32.store8
        i32.const 68
        i32.const 5
        i32.store8
        i32.const 72
        i32.const 2112
        i32.store
        i32.const 76
        i32.const 40
        i32.store
        i32.const 64
        return
      end
      local.get $c
      i32.const 119
      i32.eq
      if
        i32.const 2176
        i32.const 11
        i32.const 2208
        i32.const 21
        i32.const 16
        call $run
        i32.const 16
        return
      end
      i32.const 2048
      i32.const 16
      i32.const 2080
      i32.const 2
      i32.const 16
      call $run
      i32.const 16
    )
    (data (;0;) (i32.const 2048) "document.inspect")
    (data (;1;) (i32.const 2080) "{}")
    (data (;2;) (i32.const 2112) "the example exporter refused this design")
    (data (;3;) (i32.const 2176) "file.export")
    (data (;4;) (i32.const 2208) "{\22path\22:\22stolen.txt\22}")
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
  )
  (core module $wit-component-fixup (;2;)
    (type (;0;) (func (param i32 i32 i32 i32 i32)))
    (import "" "0" (func (;0;) (type 0)))
    (import "" "$imports" (table (;0;) 1 1 funcref))
    (elem (;0;) (i32.const 0) func 0)
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
  (type $bytes (;6;) (list u8))
  (type (;7;) (result $bytes (error $"#type5 plugin-error")))
  (type (;8;) (func (param "slot" string) (param "options" $"#type4 json") (result 7)))
  (alias core export $main "__PACKAGE__/formats-provider@__VERSION__#export-file" (core func $__PACKAGE__/formats-provider@__VERSION__#export-file (;3;)))
  (func $export-file (;1;) (type 8) (canon lift (core func $__PACKAGE__/formats-provider@__VERSION__#export-file) (memory $memory) (realloc $realloc) string-encoding=utf8))
  (instance $formats
    (export "json" (type $json))
    (export "plugin-error" (type $plugin-error))
    (export "export-file" (func $export-file)))
  (export "__PACKAGE__/formats-provider@__VERSION__" (instance $formats))
)
