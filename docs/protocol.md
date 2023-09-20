# 远端文件交互协议

## 连接建立

客户端发送 GET 请求升级 websocket 协议，服务端返回 101 Switching Protocols 响应，升级成功，或者返回 401 Unauthorized 响应，需要权限验证。

### 权限验证

权限验证采用 Basic Auth 方案，客户端发送 Authorization 请求头，格式为：

```http
Authorization: Basic <base64(account)>.<base64(passwd)>
```

服务端验证成功返回 101 Switching Protocols 响应，验证失败返回 401 Unauthorized 响应。

## 第一次交互，同步 records

主要交换 records 列表，服务端打包 records 并发送给客户端。

客户端接收到 records 列表之后，整理差异，发送给服务端，每个 record 标注缺失或新增。

## 第二次交互，同步 trees

服务端根据 records 差异中的缺失项发送对应 record 的 trees 对象列表。

客户端根据新增项发送对应 record 的 trees 对象列表，同时整理出差异 blobs，每个 blobs 标注新增或缺失。

## 第三次交互，同步 blobs

服务端根据缺失 blobs 列表发送对应 blobs 对象数据。

客户端根据新增 blobs 列表发送对应 blobs 对象数据。
