;; manifest.scm — Rust 开发环境
;;
;; 用法：
;;   guix shell -m manifest.scm
;; 或者把该文件放到项目根目录后直接运行：
;;   guix shell

(specifications->manifest
  (list
    ;; Rust 工具链
    "rust"            ; out 输出：rustc 编译器
    "rust:cargo"      ; Cargo 包管理器
    "rust:tools"      ; rustfmt 等附加开发工具
    "rust:rust-src"   ; 标准库源码（rust-analyzer / IDE 跳转需要）

    ;; 编译链接所需系统工具：提供 cc、ld 等
    "gcc-toolchain"

    ;; 常用开发依赖
    "pkg-config"      ; 供 -sys 类 crate 查找系统库
    "openssl"         ; 很多 crate（如 reqwest、git2）会用到
    "nss-certs"       ; CA 证书，让 cargo 能通过 HTTPS 拉取 crates

    ;; 调试器
    "gdb"))

