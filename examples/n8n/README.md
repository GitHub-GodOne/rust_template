# n8n 接口工作流

这里提供四份可直接导入 n8n 的工作流：

- `file-upload-single.json`：接收一个文件并调用 `POST /api/file-api/uploads`。
- `file-upload-multiple.json`：接收多个文件，拆成多个 n8n item，逐个调用单文件接口，最后汇总结果。
- `image-generation.json`：使用 n8n 内置 HTTP Request 节点调用 `POST /api/image-api/generations`。
- `image-generation-direct.json`：不使用 `base_model`，直接调用 `POST /api/image-api/images/generations`。

## 图片生成工作流

导入 `image-generation.json` 后，打开“配置生图请求”节点并替换：

- `api_base_url`：本项目后端地址。n8n 使用 Docker 时默认保持 `http://host.docker.internal:5150`。
- `platform_bearer_token`：平台永久 Bearer 密钥，只填写 `eyJ0...`，不要填写 `Bearer ` 前缀。
- `ai_api_url`：上游 AI 接口根地址。
- `ai_api_key`：上游 AI 服务密钥。
- `prompt`、`size`、`quality` 等生图参数。

保存后点击“Execute workflow”。生成结果可在“调用图片生成接口”节点中查看，常用字段为 `data.file.network_url`、`data.file.public_url` 和 `data.file.local_path`。HTTP Request 节点超时已设置为 950 秒。

如果上游支持标准 Images API，可导入 `image-generation-direct.json`。它直接请求 `/images/generations`，没有 `base_model`。工作流末尾的“整理 JSON 结果”Code 节点会把接口响应压缩为普通 JSON，首张图片地址位于 `image_url`，完整文件列表位于 `files`。n8n 的 HTTP Request 输出预览卡住时，可以直接查看这个 Code 节点的输出。图片编辑接口使用 `multipart/form-data` 调用 `/api/image-api/images/edits`，HTTP Request 节点中把输入二进制属性映射为 `image`，其余参数作为普通表单字段发送即可。

## 导入和配置

1. 在 n8n 中选择“从文件导入”，导入对应的 JSON。
2. 打开“配置接口”节点，修改：
   - `api_base_url`：后端地址，不要以 `/` 结尾。
   - `bearer_token`：在平台 API 授权管理中创建的永久 Bearer 密钥，只填写 `eyJ0...`，不要重复填写 `Bearer `。
   - `storage_bucket_id`：可选。API 密钥所属用户没有租户时必须填写，可通过 Webhook 的同名文本字段传入。
   - `prefix`：默认上传目录，也可以由 Webhook 请求中的同名文本字段覆盖。
   - `visibility`：`private` 或 `public`。
3. 保存并启用工作流，然后复制 Webhook 的 Production URL。

默认地址是 `http://host.docker.internal:5150`，适用于 macOS/Windows 上通过 Docker 运行的 n8n。如果 n8n 直接运行在宿主机，可改为 `http://127.0.0.1:5150`；如果 n8n 和后端在同一个 Docker 网络中，应改成后端服务名，例如 `http://backend:5150`。

Bearer 密钥所属用户必须拥有 `system:upload:create` 权限。为了避免密钥出现在工作流执行记录中，正式环境建议把 HTTP Request 节点改用 n8n 的 Header Auth Credential：名称填写 `Authorization`，值填写 `Bearer eyJ0...`。

如果返回 `tenant_id is required`，说明 API 密钥所属用户没有绑定租户，并且请求没有指定存储桶。可在“存储配置 -> 桶配置”表格中查看桶的 ID，然后在调用 Webhook 时增加 `storage_bucket_id`；也可以在用户管理中为 API 密钥所属用户绑定租户。

## 单文件调用

Webhook 的文件字段名必须是 `file`：

```sh
curl -X POST 'https://your-n8n.example.com/webhook/gpt-images-upload-single' \
  -F 'file=@/Users/example/Documents/report.pdf' \
  -F 'storage_bucket_id=1' \
  -F 'prefix=n8n/reports' \
  -F 'visibility=private'
```

返回示例：

```json
{
  "success": true,
  "file": {
    "id": 101,
    "original_name": "report.pdf",
    "network_url": "http://example.com/api/admin/uploads/101/download",
    "local_path": "/absolute/storage/path/n8n/reports/report.pdf",
    "object_key": "n8n/reports/report.pdf"
  }
}
```

## 多文件调用

多个文件请使用不同的 multipart 字段名，例如 `file_1`、`file_2`。工作流会把所有二进制字段统一转换成后端要求的 `file` 字段后逐个上传：

```sh
curl -X POST 'https://your-n8n.example.com/webhook/gpt-images-upload-multiple' \
  -F 'file_1=@/Users/example/Documents/report.pdf' \
  -F 'file_2=@/Users/example/Pictures/photo.png' \
  -F 'file_3=@/Users/example/Movies/demo.mp4' \
  -F 'storage_bucket_id=1' \
  -F 'prefix=n8n/batch' \
  -F 'visibility=private'
```

返回示例：

```json
{
  "success": true,
  "count": 3,
  "failed_count": 0,
  "files": [
    {
      "original_name": "report.pdf",
      "network_url": "http://example.com/api/admin/uploads/101/download",
      "local_path": "/absolute/storage/path/n8n/batch/report.pdf"
    }
  ],
  "failures": []
}
```

`network_url` 是推荐访问地址。若返回的是 `/api/admin/uploads/{id}/download` 对应地址，访问文件时仍需携带同一个 Bearer 密钥；配置了公开访问映射后，`network_url` 会优先返回公开地址。
