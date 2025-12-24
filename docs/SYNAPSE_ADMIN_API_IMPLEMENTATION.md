# Synapse Admin API Implementation

## Phase 1 Completed ✅

### Changes Made (2025-12-24)

1. **CrossSigningKey endpoint**
   - 新建迁移 `2025-12-24-050906-0000_cross_signing_uia_bypass`
   - 添加 `e2e_cross_signing_uia_bypass` 表
   - 实现 `has_master_cross_signing_key()` / `set_cross_signing_replacement_allowed()` / `get_cross_signing_replacement_allowed()`
   - 端点返回 `updatable_without_uia_before_ms`

2. **SuspendAccount endpoint**
   - 添加 `set_suspended()` 数据层函数
   - 添加 `PUT /v1/suspend/{user_id}` 端点

3. **list_users total 计数修复**
   - `list_users()` 函数 total 计数应用相同的过滤条件

4. **Admin 中间件**
   - `require_admin` 中间件验证管理员权限

## Phase 2 Completed ✅

### Changes Made (2025-12-24)

1. **UserWhois endpoint**
   - `GET /v1/whois/{user_id}` - 获取用户会话信息
   - 添加 `get_devices()` 数据层函数

2. **UserMembership endpoint**
   - `GET /v1/users/{user_id}/joined_rooms` - 获取用户加入的房间

3. **AccountData endpoint**
   - `GET /v1/users/{user_id}/accountdata` - 获取用户账户数据
   - 添加 `get_global_account_data()` / `get_room_account_data()` 数据层函数

4. **Pushers endpoint**
   - `GET /v1/users/{user_id}/pushers` - 获取用户推送器

5. **RateLimit endpoint**
   - `GET/POST/DELETE /v1/users/{user_id}/override_ratelimit`
   - 添加 `RateLimitOverride` 结构体
   - 添加 `get_ratelimit()` / `set_ratelimit()` / `delete_ratelimit()` 数据层函数

## Implemented Endpoints

### user_lookup.rs ✅
- `GET /v1/auth_providers/{provider}/users/{external_id}`
- `GET /v1/threepid/{medium}/users/{address}`

### user_admin.rs ✅
- `GET/PUT /v2/users/{user_id}` - 用户详情/创建/修改
- `GET /v2/users` - 用户列表 v2
- `GET /v3/users` - 用户列表 v3
- `POST /v1/users/{user_id}/_allow_cross_signing_replacement_without_uia`
- `POST /v1/deactivate/{user_id}` - 停用账户
- `POST /v1/reset_password/{user_id}` - 重置密码
- `GET/PUT /v1/users/{user_id}/admin` - 管理员状态
- `POST/DELETE /v1/users/{user_id}/shadow_ban` - 影子封禁
- `PUT /v1/suspend/{user_id}` - 暂停账户
- `GET /v1/whois/{user_id}` - 用户会话信息
- `GET /v1/users/{user_id}/joined_rooms` - 加入的房间
- `GET /v1/users/{user_id}/pushers` - 推送器
- `GET /v1/users/{user_id}/accountdata` - 账户数据
- `GET/POST/DELETE /v1/users/{user_id}/override_ratelimit` - 速率限制

### register.rs ✅
- `GET /username_available` - 检查用户名可用性

### user.rs (devices) ⚠️
- `GET /v2/users/{user_id}/devices/{device_id}`
- `PUT /v2/users/{user_id}/devices/{device_id}`
- `DELETE /v2/users/{user_id}/devices/{device_id}`

### room.rs ⚠️
- `GET /v1/rooms` - 房间列表
- `GET /v1/rooms/{room_id}/hierarchy` - 房间层级

## TODO

### Phase 3: Device Management
- `GET /v2/users/{user_id}/devices` - 设备列表
- `POST /v2/users/{user_id}/delete_devices` - 批量删除设备

### Phase 4: Room Management
- `GET /v1/rooms/{room_id}` - 房间详情
- `DELETE /v2/rooms/{room_id}` - 删除房间
- `GET /v1/rooms/{room_id}/members` - 房间成员
- `GET /v1/rooms/{room_id}/state` - 房间状态

### Low Priority
- Federation management
- Media management
- Statistics
- Event reports
- Server notices
- Registration tokens

## Code Structure

```
crates/server/src/routing/admin/
├── mod.rs           # 路由汇总 + require_admin 中间件
├── register.rs      # ✅ username_available
├── room.rs          # ⚠️ 部分实现
├── user.rs          # ⚠️ 设备管理
├── user_admin.rs    # ✅ 用户管理 API
└── user_lookup.rs   # ✅ MAS 查找 API

crates/data/src/user/
├── device.rs        # get_device, get_devices, remove_device...
├── data.rs          # get_global_account_data, get_room_account_data...
└── key.rs           # has_master_cross_signing_key, set/get_cross_signing_replacement_allowed

crates/data/src/user.rs
└── RateLimitOverride, get_ratelimit, set_ratelimit, delete_ratelimit
```

## Testing

```bash
cargo check -p palpo
cargo build && ./target/debug/palpo

# Test endpoints
curl -X GET "http://localhost:8008/_synapse/admin/v2/users?limit=10" \
  -H "Authorization: Bearer $ADMIN_TOKEN"

curl -X GET "http://localhost:8008/_synapse/admin/v1/whois/@user:example.com" \
  -H "Authorization: Bearer $ADMIN_TOKEN"
```
