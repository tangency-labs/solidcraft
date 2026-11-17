;; Original importer test component, encoded from the importer-world WIT with wasm-tools 1.263.0.
;; It copies the start of the file it imports into the design and returns a one-item loss report.
;; A file starting with S copies part.bin beside it instead, T traps and L loops forever.
(component
  (type $ty-__PACKAGE__/types@__VERSION__ (;0;)
    (instance
      (type (;0;) (record (field "command" string) (field "reason" string)))
      (export (;1;) "command-failure" (type (eq 0)))
      (type (;2;) (record (field "field" string) (field "reason" string)))
      (export (;3;) "field-failure" (type (eq 2)))
      (type (;4;) (variant (case "permission-denied" string) (case "command-failed" 1) (case "limit-exceeded" string) (case "invalid-manifest" 3) (case "invalid-input" 3) (case "trap" string) (case "cancelled")))
      (export (;5;) "plugin-error" (type (eq 4)))
      (type (;6;) string)
      (export (;7;) "json" (type (eq 6)))
    )
  )
  (import "__PACKAGE__/types@__VERSION__" (instance $__PACKAGE__/types@__VERSION__ (;0;) (type $ty-__PACKAGE__/types@__VERSION__)))
  (alias export $__PACKAGE__/types@__VERSION__ "plugin-error" (type $plugin-error (;1;)))
  (type $ty-__PACKAGE__/source@__VERSION__ (;2;)
    (instance
      (alias outer 1 $plugin-error (type (;0;)))
      (export (;1;) "plugin-error" (type (eq 0)))
      (type (;2;) (record (field "name" string) (field "size" u64)))
      (export (;3;) "file-info" (type (eq 2)))
      (type (;4;) (list u8))
      (type (;5;) (result 4 (error 1)))
      (type (;6;) (func (param "path" string) (param "offset" u64) (param "len" u32) (result 5)))
      (export (;0;) "read" (func (type 6)))
    )
  )
  (import "__PACKAGE__/source@__VERSION__" (instance $__PACKAGE__/source@__VERSION__ (;1;) (type $ty-__PACKAGE__/source@__VERSION__)))
  (alias export $__PACKAGE__/types@__VERSION__ "plugin-error" (type $"#type3 plugin-error" (@name "plugin-error") (;3;)))
  (type $ty-__PACKAGE__/design@__VERSION__ (;4;)
    (instance
      (alias outer 1 $"#type3 plugin-error" (type (;0;)))
      (export (;1;) "plugin-error" (type (eq 0)))
      (type (;2;) (list u8))
      (type (;3;) (result (error 1)))
      (type (;4;) (func (param "bytes" 2) (result 3)))
      (export (;0;) "write" (func (type 4)))
    )
  )
  (import "__PACKAGE__/design@__VERSION__" (instance $__PACKAGE__/design@__VERSION__ (;2;) (type $ty-__PACKAGE__/design@__VERSION__)))
  (core module $main (;0;)
    (type (;0;) (func (param i32 i32 i64 i32 i32)))
    (type (;1;) (func (param i32 i32 i32)))
    (type (;2;) (func (param i32 i32 i32 i32) (result i32)))
    (type (;3;) (func (param i32 i32 i32 i32 i64 i32 i32) (result i32)))
    (import "__PACKAGE__/source@__VERSION__" "read" (func $read (;0;) (type 0) (param i32 i32 i64 i32 i32)))
    (import "__PACKAGE__/design@__VERSION__" "write" (func $write (;1;) (type 1) (param i32 i32 i32)))
    (memory (;0;) 1)
    (global $heap (;0;) (mut i32) i32.const 8192)
    (export "memory" (memory 0))
    (export "cabi_realloc" (func $realloc))
    (export "__PACKAGE__/importer@__VERSION__#import-file" (func $import))
    (func $realloc (;2;) (type 2) (param i32 i32 i32 i32) (result i32)
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
    (func $import (;3;) (type 3) (param i32 i32 i32 i32 i64 i32 i32) (result i32)
      i32.const 0
      i32.const 0
      i64.const 0
      i32.const 65536
      i32.const 16
      call $read
      i32.const 16
      i32.load8_u
      if ;; label = @1
        i32.const 16
        return
      end
      i32.const 20
      i32.load
      i32.load8_u
      i32.const 84
      i32.eq
      if ;; label = @1
        unreachable
      end
      i32.const 20
      i32.load
      i32.load8_u
      i32.const 76
      i32.eq
      if ;; label = @1
        loop $forever
          br $forever
        end
      end
      i32.const 20
      i32.load
      i32.load8_u
      i32.const 83
      i32.eq
      if ;; label = @1
        i32.const 2048
        i32.const 8
        i64.const 0
        i32.const 65536
        i32.const 16
        call $read
        i32.const 16
        i32.load8_u
        if ;; label = @2
          i32.const 16
          return
        end
      end
      i32.const 20
      i32.load
      i32.const 24
      i32.load
      i32.const 48
      call $write
      i32.const 48
      i32.load8_u
      if ;; label = @1
        i32.const 48
        return
      end
      i32.const 64
      i32.const 0
      i32.store8
      i32.const 68
      i32.const 1024
      i32.store
      i32.const 72
      i32.const 120
      i32.store
      i32.const 64
    )
    (data (;0;) (i32.const 1024) "{\22summary\22:\22Copied the file\22,\22items\22:[{\22severity\22:\22dropped\22,\22subject\22:\22Sketch1\22,\22message\22:\22Splines are not supported\22}]}")
    (data (;1;) (i32.const 2048) "part.bin")
    (@producers
      (processed-by "wit-component" "0.263.0")
    )
  )
  (core module $wit-component-shim-module (;1;)
    (type (;0;) (func (param i32 i32 i64 i32 i32)))
    (type (;1;) (func (param i32 i32 i32)))
    (table (;0;) 2 2 funcref)
    (export "0" (func $indirect-__PACKAGE__/source@__VERSION__-read))
    (export "1" (func $indirect-__PACKAGE__/design@__VERSION__-write))
    (export "$imports" (table 0))
    (func $indirect-__PACKAGE__/source@__VERSION__-read (;0;) (type 0) (param i32 i32 i64 i32 i32)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      local.get 4
      i32.const 0
      call_indirect (type 0)
    )
    (func $indirect-__PACKAGE__/design@__VERSION__-write (;1;) (type 1) (param i32 i32 i32)
      local.get 0
      local.get 1
      local.get 2
      i32.const 1
      call_indirect (type 1)
    )
    (@producers
      (processed-by "wit-component" "0.263.0")
    )
  )
  (core instance $wit-component-shim-instance (;0;) (instantiate $wit-component-shim-module))
  (alias export $__PACKAGE__/types@__VERSION__ "json" (type $json (;5;)))
  (alias export $__PACKAGE__/types@__VERSION__ "plugin-error" (type $"#type6 plugin-error" (@name "plugin-error") (;6;)))
  (alias export $__PACKAGE__/source@__VERSION__ "file-info" (type $file-info (;7;)))
  (alias core export $wit-component-shim-instance "0" (core func $indirect-__PACKAGE__/source@__VERSION__-read (;0;)))
  (core instance $__PACKAGE__/source@__VERSION__ (;1;)
    (export "read" (func $indirect-__PACKAGE__/source@__VERSION__-read))
  )
  (alias core export $wit-component-shim-instance "1" (core func $indirect-__PACKAGE__/design@__VERSION__-write (;1;)))
  (core instance $__PACKAGE__/design@__VERSION__ (;2;)
    (export "write" (func $indirect-__PACKAGE__/design@__VERSION__-write))
  )
  (core instance $main (;3;) (instantiate $main
      (with "__PACKAGE__/source@__VERSION__" (instance $__PACKAGE__/source@__VERSION__))
      (with "__PACKAGE__/design@__VERSION__" (instance $__PACKAGE__/design@__VERSION__))
    )
  )
  (alias core export $main "memory" (core memory $memory (;0;)))
  (core module $wit-component-fixup (;2;)
    (type (;0;) (func (param i32 i32 i64 i32 i32)))
    (type (;1;) (func (param i32 i32 i32)))
    (import "actual" "0" (func $0 (;0;) (type 0) (param i32 i32 i64 i32 i32)))
    (import "actual" "1" (func $1 (;1;) (type 1) (param i32 i32 i32)))
    (import "shim" "$imports" (table (;0;) 2 2 funcref))
    (elem (;0;) (i32.const 0) func $0 $1)
    (@producers
      (processed-by "wit-component" "0.263.0")
    )
  )
  (alias export $__PACKAGE__/source@__VERSION__ "read" (func $read (;0;)))
  (alias core export $main "cabi_realloc" (core func $cabi_realloc (;2;)))
  (core func $"#core-func3 indirect-__PACKAGE__/source@__VERSION__-read" (@name "indirect-__PACKAGE__/source@__VERSION__-read") (;3;) (canon lower (func $read) (memory $memory) (realloc $cabi_realloc) string-encoding=utf8))
  (alias export $__PACKAGE__/design@__VERSION__ "write" (func $write (;1;)))
  (core func $"#core-func4 indirect-__PACKAGE__/design@__VERSION__-write" (@name "indirect-__PACKAGE__/design@__VERSION__-write") (;4;) (canon lower (func $write) (memory $memory) (realloc $cabi_realloc) string-encoding=utf8))
  (core instance $actual (;4;)
    (export "0" (func $"#core-func3 indirect-__PACKAGE__/source@__VERSION__-read"))
    (export "1" (func $"#core-func4 indirect-__PACKAGE__/design@__VERSION__-write"))
  )
  (core instance $fixup (;5;) (instantiate $wit-component-fixup
      (with "actual" (instance $actual))
      (with "shim" (instance $wit-component-shim-instance))
    )
  )
  (type (;8;) (result $json (error $"#type6 plugin-error")))
  (type (;9;) (func (param "entry" string) (param "file" $file-info) (param "options" $json) (result 8)))
  (alias core export $main "__PACKAGE__/importer@__VERSION__#import-file" (core func $__PACKAGE__/importer@__VERSION__#import-file (;5;)))
  (func $import-file (;2;) (type 9) (canon lift (core func $__PACKAGE__/importer@__VERSION__#import-file) (memory $memory) (realloc $cabi_realloc) string-encoding=utf8))
  (alias export $__PACKAGE__/types@__VERSION__ "command-failure" (type $command-failure (;10;)))
  (alias export $__PACKAGE__/types@__VERSION__ "field-failure" (type $field-failure (;11;)))
  (component $__PACKAGE__/importer@__VERSION__-shim-component (;0;)
    (type (;0;) string)
    (import "import-type-json" (type (;1;) (eq 0)))
    (type (;2;) (record (field "command" string) (field "reason" string)))
    (import "import-type-command-failure" (type (;3;) (eq 2)))
    (type (;4;) (record (field "field" string) (field "reason" string)))
    (import "import-type-field-failure" (type (;5;) (eq 4)))
    (type (;6;) (variant (case "permission-denied" string) (case "command-failed" 3) (case "limit-exceeded" string) (case "invalid-manifest" 5) (case "invalid-input" 5) (case "trap" string) (case "cancelled")))
    (import "import-type-plugin-error" (type (;7;) (eq 6)))
    (type (;8;) (record (field "name" string) (field "size" u64)))
    (import "import-type-file-info" (type (;9;) (eq 8)))
    (import "import-type-file-info0" (type (;10;) (eq 9)))
    (import "import-type-json0" (type (;11;) (eq 1)))
    (import "import-type-plugin-error0" (type (;12;) (eq 7)))
    (type (;13;) (result 11 (error 12)))
    (type (;14;) (func (param "entry" string) (param "file" 10) (param "options" 11) (result 13)))
    (import "import-func-import-file" (func (;0;) (type 14)))
    (export (;15;) "json" (type 1))
    (export (;16;) "plugin-error" (type 7))
    (export (;17;) "file-info" (type 9))
    (type (;18;) (result 15 (error 16)))
    (type (;19;) (func (param "entry" string) (param "file" 17) (param "options" 15) (result 18)))
    (export (;1;) "import-file" (func 0) (func (type 19)))
  )
  (instance $__PACKAGE__/importer@__VERSION__-shim-instance (;3;) (instantiate $__PACKAGE__/importer@__VERSION__-shim-component
      (with "import-func-import-file" (func $import-file))
      (with "import-type-json" (type $json))
      (with "import-type-command-failure" (type $command-failure))
      (with "import-type-field-failure" (type $field-failure))
      (with "import-type-plugin-error" (type $"#type6 plugin-error"))
      (with "import-type-file-info" (type $file-info))
      (with "import-type-file-info0" (type $file-info))
      (with "import-type-json0" (type $json))
      (with "import-type-plugin-error0" (type $"#type6 plugin-error"))
    )
  )
  (export $__PACKAGE__/importer@__VERSION__ (;4;) "__PACKAGE__/importer@__VERSION__" (instance $__PACKAGE__/importer@__VERSION__-shim-instance))
  (@producers
    (processed-by "wit-component" "0.263.0")
  )
)
