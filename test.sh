#!/usr/bin/env bash
set -euo pipefail

BASE_URL='https://dm-fox.rjj.cc/codex/v1'
API_KEY='sk-ant-oat01-PKGDxwh1izK22864AabgzQDO4GtbR4LCLhQyFDje9tckB6IcKVUdQF8SlHEXCcXnIyKHQdgMzRnY6QczqW2_ilIAPn9ayAA'
OUT_DIR='/tmp/ai-image-test'
mkdir -p "$OUT_DIR"

cat > "$OUT_DIR/request.json" <<'JSONEOF'
{
  "model": "gpt-image-2",
  "prompt": "国风戏曲仙侠彩妆，冷白瓷肌，超大眼妆设计，红粉紫渐变眼影，凤凰羽翼彩绘，火焰纹样面部彩绘，额间花钿，水钻装饰，粉嫩唇妆，东方古典美人，精致盘发，红色流苏发饰，梦幻仙气，专业彩妆大赛作品，二次元与戏曲融合美学，对称构图，超精细细节，高清特写摄影，cinematic lighting, masterpiece, beauty competition makeup, highly detailed, 8k。",
  "size": "1024x1536",
  "quality": "high",
  "n": 1
}
JSONEOF

cat > "$OUT_DIR/curl.txt" <<EOF
curl --location --request POST '$BASE_URL/images/generations' \
  --header 'Authorization: Bearer $API_KEY' \
  --header 'Content-Type: application/json' \
  --data-binary @'$OUT_DIR/request.json'
EOF

status_code="$(curl --location --request POST "$BASE_URL/images/generations" \
  --header "Authorization: Bearer $API_KEY" \
  --header "Content-Type: application/json" \
  --data-binary @"$OUT_DIR/request.json" \
  -D "$OUT_DIR/response.headers" \
  -o "$OUT_DIR/response.body" \
  -w "%{http_code}" \
  -sS)"

printf '%s\n' "$status_code" | tee "$OUT_DIR/http_code.txt"

if [[ "$status_code" != 2* ]]; then
  printf '\n--- response.headers ---\n'
  cat "$OUT_DIR/response.headers"
  printf '\n--- response.body ---\n'
  cat "$OUT_DIR/response.body"
  printf '\n--- request.json ---\n'
  cat "$OUT_DIR/request.json"
  printf '\n--- curl.txt ---\n'
  cat "$OUT_DIR/curl.txt"
fi
