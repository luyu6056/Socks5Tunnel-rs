# Socks5Tunnel-rs
## 简介

一个加密的socks5隧道，client与伪装成mysql的server连接，应用程序通过socks5连接到client穿透到server外网。

## 用法

修改rust-server/main.rs, let addr = "0.0.0.0:3304"; 改为server监听的端口
修改rust-client/main.rs, const ADDR: &str = "127.0.0.1:3304"; 改为上述server的公网 ip和端口

## 注意
为了使用自签证书，开启了以下设定
```rust
//rust-client/main.rs ,line 86
            .use_sni(false)
            .danger_accept_invalid_certs(true)
```
如果你使用可以通过ca校验的证书，可以尝试开启sni，把**danger_accept_invalid_certs**设置为**false**