# BCS Session Collection (收藏) 设计

- 日期: 2026-07-15
- 模块: `src/bcs` (BCS — Bot Coordination Service)
- 状态: v2（按评审修订：uncollect 404 语义统一、SQLite 既有库迁移闭环、新增 repo conformance 测试）

## 1. 背景 / 动机

一个 group 下实例化的 session 数量可能很多，bot 需要能够按自身维度收藏部分
session，以便管理与查找。当前唯一获取 session 列表的接口为:

```
GET /groups/{groupId}/sessions?limit=20&offset=0&participant={botUuid}
```

需要扩展支持「按 bot 收藏」语义：collection 是 **bot 拥有的元数据**，作用域为
`(env, group_id, session_id, bot_uuid)`。

## 2. 目标 / 非目标

**目标**

- bot 可以对 group 内属于自己的 session 执行 collect / uncollect。
- 按 bot 维度列出其在该 group 内已收藏的 session。
- 改动最小化，复用现有 session 列表路径与会话成员表结构。

**非目标**

- 不引入排序 / 置顶。
- 不做跨 group 的收藏汇总。
- 不改动 session 本身的领域模型与持久化结构。

## 3. 关键决策（与用户确认）

| 维度 | 选择 |
| --- | --- |
| 功能范围 | collect / uncollect / list（完整闭环） |
| 列表接口形式 | 复用 `GET /groups/{groupId}/sessions?participant={botUuid}&collected=true` |
| 写接口形式 | `POST /sessions/{sid}/collect` + `DELETE /sessions/{sid}/collect` |
| 收藏者身份解析 | bot token → 该 bot；human cookie → 显式 `participant`（需为 owned bot，复用 create-session 的所有权校验） |
| 可收藏范围 | 必须是该 session 的参与者（侧表有行）方可 collect；非参与者 collect → `404`。选此口径而非「group 可见即可」，理由：收藏标记本就挂在 `(session, participant)` 行上，离开 / 未参与即无行可挂 |
| 实现方案 | 方案 B：收藏 fold 进现有 session 管理（`SessionManagementService` + `SessionRepoPort`） |
| 收藏存储 | 复用 `bcs_session_participants` 侧表，新增 `collected` 列；不改动领域 `Participant` / `participants` JSON 列 |
| Trait 扩展形态 | B-ii：新增独立方法 + 默认实现，避免改动 ~15 处 `list_by_group` 签名与测试 mock |

## 4. 数据模型

### 4.1 表结构变更

在 `bcs_session_participants` 侧表新增一列：

```sql
ALTER TABLE `bcs_session_participants`
  ADD COLUMN `collected` tinyint(4) NOT NULL DEFAULT '0';
CREATE INDEX `idx_collected`
  ON `bcs_session_participants` (`env`, `group_id`, `bot_uuid`, `collected`);
```

类型选 `tinyint(4)` 是与仓库现有布尔列惯例一致（`is_creator`、`is_deleted`、
`auto_start_on_service_invocation`、`disabled` 等均为 `tinyint(4) NOT NULL DEFAULT '0'`，
0/1 语义），不用 `BOOLEAN`/`TINYINT(1)`，避免与既有 schema 风格不一致。

理由：collection 语义上就是 `(session, participant)` 的属性，侧表已有
`(env, session_id, bot_uuid)` 主键与 `(env, bot_uuid)` 索引，新增一列即可表达，
无需新表。

### 4.2 领域模型不变

`bcs_domain::group::Participant` 与 `Session.participants` JSON 列**保持不变**。
`collected` 不进入 `Participant` struct：

- MySQL store：`collected` 仅存于侧表，列表/读写通过 JOIN 或侧表 UPDATE 完成。
- Memory store：侧表不存在，用独立 `HashSet<(session_id, bot_uuid)>` 记录收藏。
- 测试 mock：通过 trait 默认实现获得 no-op 行为，无需感知 `collected`。

### 4.3 既有边界行为（已验证，接受）

- `remove_participant` 执行 `DELETE FROM bcs_session_participants` → 离开
  session 即丢弃收藏标记。
- `add_participant` 使用 `ON CONFLICT DO NOTHING`，已存在时不 upsert → 仍在场
  时重新加入不会重置 `collected`；但若先 leave 再 rejoin（行已被删除），新插入
  行 `collected` 回到默认值 0。

以上均为符合直觉的行为（离开即失 bookmark）。

## 5. 架构 / 分层

严格遵循 BCS 分层（delivery adapter → application → core/port → store）：

```
HTTP  routes/sessions.rs
   ├─ list_sessions_for_group
   │     collected=true → session_management.list_collected_by_group(...)
   │     否则           → session_management.list_by_group(...)   （不变）
   ├─ POST   /sessions/{sid}/collect   → session_management.collect(sid, bot)
   └─ DELETE /sessions/{sid}/collect   → session_management.uncollect(sid, bot)

SessionManagementService (application/session.rs)   [新增方法 + 默认实现]
   ├─ collect(session_id, bot_uuid)            校验 session 存在 → repo.collect(...)
   ├─ uncollect(session_id, bot_uuid)          → repo.uncollect(...)
   └─ list_collected_by_group(group, bot, status, q, offset, limit)
                                                → repo.list_collected_by_group(...)

SessionRepoPort (port/repo/session.rs)   [新增方法 + 默认实现]
   ├─ collect(session_id, bot_uuid)
   ├─ uncollect(session_id, bot_uuid)
   └─ list_collected_by_group(group_id, bot_uuid, status, q, offset, limit)
   impls: MySqlSessionStore, MemorySessionRepo  （+ RecordingSessionRepo / 测试 mock 走默认实现）
```

未触碰 core `*CoreService`：collection 是纯持久化 + 校验，无跨域编排，不需要
core 层。application 层负责 session 存在性 / 归属校验后委托 repo。

## 6. HTTP 接口契约

### 6.1 列出已收藏 session

```
GET /groups/{groupId}/sessions?participant={botUuid}&collected=true
```

- 复用现有路由与 `list_sessions_for_group` handler。
- 新增 query 字段 `collected: Option<bool>`。
- 当 `collected=true` 时：
  - `participant` 为必填，否则 `400 { "error": "invalid_params", "message": "collected filter requires participant" }`。
  - 调用 `list_collected_by_group`，返回该 bot 在本 group 已收藏的 session
    （仍支持 `status` / `q` / `offset` / `limit` 叠加）。
  - 响应体与现有列表完全一致：`{ "items": [...], "group_id": "..." }`，
    每个 item 经 `session_to_json`（含 `id` + `session_id` 双键）。
- 当 `collected` 不为 true（缺省 / false）：走原 `list_by_group`，行为不变。
- collected 列表分支**不做** legacy auto-create（自动建 `{group}:00000000`）逻辑，
  该逻辑仅服务于「群内完全无 session」的旧群迁移，与收藏过滤无关。
- **有意取舍（已确认接受）**：普通列表（`participant=B` 不带 `collected=true`）
  的每个 item **不**返回 `collected` 标记，响应体保持与现有列表完全一致。这与
  §2「改动最小化」一致；副作用是前端若要在普通列表里高亮已收藏项，需对已收藏
  session 再请求一次（或后续按需扩展 item 携带 `collected` 字段，属 YAGNI）。

### 6.2 收藏 session

```
POST /sessions/{sid}/collect
```

- 无 body。`Authorization: Bearer <token>` 或 human cookie。
- 调用者解析（复用 `resolve_group_chat_caller`）：
  - Bot token → collector = 该 bot。
  - Human cookie → body 可带 `participant` 字段指定 bot_uuid；校验 human 拥有该
    bot（`registry.list_bots_by_creator`），与 create-session 的 `created_by` 解析
    一致。human 未带 `participant` → `400`。
- 应用层校验：session 存在；bot 是该 session 参与者（侧表有行），否则 `404`。
- 成功返回 `200 { "collected": true, "session_id": "..." }`。
- 已收藏 → 幂等返回 `200`，不报错。
- 鉴权失败 `401`；非参与者 `404`。

### 6.3 取消收藏

```
DELETE /sessions/{sid}/collect
```

- 同 6.2 的调用者解析（DELETE 带 body 不通用，collector 通过 `participant`
  query 参数指定；bot token 则忽略）。
- **uncollect 始终幂等**：应用层先 `get(sid)` 确认 session 存在（不存在 → `404`）；
  随后执行 UPDATE，无论目标行是否存在（非参与者 / 本就未收藏）一律返回
  `200 { "collected": false, "session_id": "..." }`。
- 即 uncollect 的唯一 404 是 session 不存在；非参与者 / 未收藏均幂等 200。
- 鉴权失败 `401`；session 不存在 `404`。

## 7. 权限 / 鉴权

- `resolve_group_chat_caller(&state, &headers, &uri)` 解析 Bot token 或 Human
  cookie（与 group-chat 路由一致）。
- Bot token：collector 即该 bot。
- Human cookie：必须显式提供 `participant=botUuid`；通过
  `registry.list_bots_by_creator(&staff_no)` 校验 ownership，不通过 → `403`。
- collect / uncollect 路径为 `/sessions/{sid}`（不含 groupId），故应用层仅做
  `get(sid)` 确认 session 存在（不存在 → 404）；权限完全由调用者鉴权决定，
  无需 `belongs_to_group`。`Session.group_id` 仅用于返回值 / 日志。

## 8. Trait 定义（新增）

`port/repo/session.rs` 新增（带默认实现，不破坏现有 impl）：

```rust
async fn collect(&self, _session_id: &str, _bot_uuid: &str) -> ServiceResult<()> {
    Err(ServiceError::InternalError("collect not implemented".into()))
}
async fn uncollect(&self, _session_id: &str, _bot_uuid: &str) -> ServiceResult<()> {
    Err(ServiceError::InternalError("uncollect not implemented".into()))
}
async fn list_collected_by_group(
    &self,
    _group_id: &str,
    _bot_uuid: &str,
    _status: Option<SessionStatus>,
    _title_contains: Option<&str>,
    _offset: u64,
    _limit: u64,
) -> Vec<Session> {
    Vec::new()
}
```

`application/session.rs` `SessionManagementService` 对应新增三个方法，同样带
默认实现（返回 `Err(SessionUseCaseError::Internal(...))` / 空列表）。
真实实现仅写在 `bcs-session/src/application.rs`（委托 repo）。

错误映射：`ServiceError::SessionNotFound` → `SessionUseCaseError::NotFound`；
其余走既有 `From<ServiceError>`。

> **默认实现的取舍（已确认接受）**：现有 `SessionRepoPort` 方法全是纯签名、无默认
> 体，memory/mysql 两端全量实现。新方法加「默认返回 error / 空列表」是 B-ii 为
> 避免改动 ~15 处测试 mock 的有意取舍。代价是：新实现若忘了 override 不会编译报错，
> 而是运行期静默返回 "not implemented"。arch.rules 强调「取决于声明契约而非具体实现」，
> 故以两条兜底：(1) `bcs-session-store/tests/conformance_session_repo.rs` 必须覆盖
> 新契约方法（见 §13.4），新实现不过契约测即红；(2) §8 头部已显式标注这些方法的
> 真实实现位置（mysql + memory + application.rs）。

## 9. MySQL 实现要点

### 9.1 collect / uncollect

```sql
-- collect
UPDATE bcs_session_participants
SET collected = 1
WHERE env = ? AND session_id = ? AND bot_uuid = ?;
-- affected_rows == 0 → ServiceError::SessionNotFound("participant not in session")
```

`uncollect` 同理 `SET collected = 0`。与 collect 不同，**uncollect 始终幂等**：

- 应用层先 `get(sid)` 确认 session 存在，不存在 → `SessionNotFound`（404）。
- 随后执行 UPDATE；无论 affected_rows 为 0（participant 不在 / 本就未收藏）
  还是为 1，均返回 `Ok(())`（200）。

即：uncollect 的唯一 404 来源于 session 不存在；非 participant 或未收藏一律
幂等 200，与 6.3「未收藏幂等」一致。

### 9.2 list_collected_by_group

复用 `list_by_group` 的 JOIN 思路：

```sql
SELECT {select_cols_prefixed}
FROM bcs_group_sessions s
JOIN bcs_session_participants sp
  ON sp.env = s.env AND sp.session_id = s.session_id
WHERE s.env = ?
  AND s.group_id = ?
  AND sp.group_id = ?          -- 与 s.group_id 同值；补此条件以命中 idx_collected
  AND sp.bot_uuid = ?
  AND sp.collected = 1
  [AND s.status = ?]            -- 可选
  [AND s.session_title LIKE ?]  -- 可选
ORDER BY s.gmt_create DESC
LIMIT ? OFFSET ?
```

封装为一个独立方法，内部与 `list_by_group` 共享 `select_cols_prefixed` /
参数拼装风格，逻辑无重复（参数都来自同一组条件构造）。

> 索引对齐：§4.1 的 `idx_collected(env, group_id, bot_uuid, collected)` 全部
> 前导列在这条查询里都被等值过滤，因此补 `AND sp.group_id = ?` 是为了让 SQLite/MySQL
> 都能走该索引——本功能核心场景正是「session 多、按 bot 快速筛收藏」，索引命中值得做对。

> 注：内存实现等价物在 §10 给出；两侧表/不表实现差异由 store 内部吸收，
> application 层只依赖 trait 契约。

## 10. Memory 实现要点

`MemorySessionRepo` state 内新增 `collected: Arc<RwLock<HashSet<(String, String)>>>`
（key = `(session_id, bot_uuid)`）。

- `collect`：先确认 `session.participants` 含 bot，否则
  `SessionNotFound`；写入 set。（幂等：set 天然去重）
- `uncollect`：从 set 移除。（幂等：`remove` 不存在不报错）→ 返回 `Ok(())`。
- `list_collected_by_group`：遍历 set 中 session_id 属于该 group 的项，
  反查 `sessions` 表过滤 `status` / `title`，按 `created_at` desc 排序后分页。
- session 不存在时（set 残留）跳过并清理该条。

## 11. 路由注册

`router.rs` 现有 `/sessions/{sid}` 子路由组新增两条：

```rust
.route("/sessions/{sid}/collect",
    post(routes::sessions::collect_session)
        .delete(routes::sessions::uncollect_session))
```

紧邻现有 `/sessions/{sid}/members` 等路由。

## 12. 错误处理

| 场景 | HTTP |
| --- | --- |
| 未鉴权 | 401 |
| session 不存在 | 404 |
| bot 非 session 参与者（collect 时） | 404 |
| 非参与者 / 未收藏时 uncollect | 200（幂等，非 404） |
| human 无权操作目标 bot | 403 |
| `collected=true` 缺 participant | 400 |
| 已收藏 / 未收藏重复操作 | 200（幂等） |
| store 内部错误 | 500 |

`session_error_to_response` 复用于 `SessionUseCaseError`；新增的 invalid_params
场景在 handler 内构造 `400`。

## 13. 测试

### 13.1 Memory store 单测（`bcs-session-store/src/memory.rs` 模块测试）

- collect 后 `list_collected_by_group` 返回该 session。
- uncollect 后不再返回。
- 非 participant collect → `SessionNotFound`。
- 重复 collect 幂等；重复 uncollect 幂等。
- 与 `status` / `title` 过滤叠加。
- `remove_participant` 后收藏标记消失（leave 即丢）。

### 13.2 HTTP 契约测试（新文件 `session_collection_contract.rs`）

使用既有 test-support 的 mock `SessionManagementService` 模式：

- `GET ...?participant=B&collected=true` 返回已收藏列表；不传 participant → 400。
- `POST /sessions/{sid}/collect` 成功；重复幂等。
- `DELETE /sessions/{sid}/collect` 成功；重复幂等。
- bot token 与 human（带 owned participant）两种调用者路径。
- human 指定非自有 bot → 403。
- 非 participant session → 404。

### 13.3 既有测试不回归

- `session_list_contract.rs` 完全不动，证明不加 `collected` 时路径行为不变。
- `SessionManagementService` / `SessionRepoPort` 既有 mock 仅靠默认实现即可编译，
  无需改动。

### 13.4 repo 级 conformance 测试（契约覆盖，必须）

新增 `collect/uncollect/list_collected_by_group` 进了 `SessionRepoPort`——这是
持久层**契约**。按 `docs/arch/arch.rules.md`「conformance tests validate
implementations against current contract definitions」，新方法必须由 repo 契约
harness 覆盖，使 MySQL 与 Memory 两个实现按同一契约被验证（HTTP 契约测只走 memory
backend，不能替代对 MySQL 的约束）。

扩展 `crates/test-support/bcs-test-support/src/contract/repo/mod.rs` 中的
`session_repo_port_contract_tests`，追加断言：

- collect 前后 `list_collected_by_group` 返回变化（出现该 session）。
- uncollect 后 `list_collected_by_group` 不再返回。
- 非 participant collect → `SessionNotFound`（`Err`）。
- uncollect 对未收藏 / 非参与者幂等（`Ok(())`）。
- 与 `status` / `title` 过滤叠加。
- collect 与 `participant` 过滤交互：仅当 `sp.bot_uuid` 指定者收藏。

`bcs-session-store/tests/conformance_session_repo.rs` 调用该 harness 即自动覆盖
MySQL + Memory 两端，无需新写 conformance 入口。这也正是 §8 默认实现取舍的兜底
机制——新实现若未正确 override，该 harness 会红。

## 14. 迁移

MySQL 与 SQLite 两条路径都要闭环，**仅追加 version 记录 + 改 CREATE TABLE 对既有
SQLite 库是空操作**（已核实：`apply_sqlite_migration_body` 用 `match migration.version`
分派，version 3 即 `Ok(())`，无 `4 =>` 分支会落到 `_ => Ok(())`，既有库不会被补列）。

### 14.1 MySQL

- 新增 `src/bcs/migrations/mysql/004_add_session_collection.sql`：

```sql
ALTER TABLE `bcs_session_participants`
  ADD COLUMN IF NOT EXISTS `collected` tinyint(4) NOT NULL DEFAULT '0';
CREATE INDEX `idx_collected`
  ON `bcs_session_participants` (`env`, `group_id`, `bot_uuid`, `collected`);
```

`ADD COLUMN IF NOT EXISTS` 与现有 `002_add_owner_bot_id.sql` 同款写法，既有库 /
新库均安全。

### 14.2 SQLite（新库 + 既有库双路径）

1. **新库**：在 `bootstrap/bcs/src/migrations.rs` 的 `bcs_session_participants`
   CREATE TABLE 语句追加 `collected INTEGER NOT NULL DEFAULT 0` 列。
2. **既有库补列**：仿 `ensure_sqlite_message_owner_bot_id`（`migrations.rs:780`）
   先例，新增 `ensure_sqlite_session_collected_column(db)`：用
   `PRAGMA table_info(bcs_session_participants)` 检查缺列再
   `ALTER TABLE ... ADD COLUMN collected INTEGER NOT NULL DEFAULT 0`，并
   `CREATE INDEX IF NOT EXISTS idx_collected ...`。在 `run_sqlite_bootstrap_tables`
   中调用（与 owner_bot_id 同处），保证每次启动对既有库幂等补列。
3. **版本登记**：`SQLITE_VERSIONED_MIGRATIONS` 追加
   `{ version: 4, name: "add_session_collection" }`；在
   `apply_sqlite_migration_body` 的 `match` 增加
   `4 => Ok(())`（实际补列已由步骤 2 的 ensure 函数在 bootstrap 完成，version 4
   仅作版本登记，语义与 version 3 `add_organizations` 一致）。

> 关键：步骤 2 是既有库闭环的必要条件。只做步骤 1+3 会让既有 SQLite 库的
> `bcs_session_participants` 缺 `collected` 列，collect/uncollect 的 ALTER/SELECT
> 在运行期失败。

## 15. 风险 / 取舍

- **方案 B-ii 的轻微重复**：`list_collected_by_group` 复制 `list_by_group` 的
  参数拼装样式。已通过共享 `select_cols_prefixed` 与相同条件构造降低重复。
- **侧表耦合**：收藏标记与参与关系同表，leave 即丢。是有意行为；若未来需
  「离开后仍保留收藏」需另起表，属 YAGNI。
- **Memory store 双写**：memory 无侧表，需在 store 内维护独立 set；与 MySQL
  侧表语义对齐，由单测保证。
- **contract test mock 不感知 collected**：依赖默认实现返回空 / 报错；真实
  collection 行为由 memory store 单测 + HTTP 契约测（用 memory backend）覆盖。

## 16. 不在本次范围

- 收藏排序 / 置顶。
- 跨 group 收藏视图。
- 收藏通知 / 系统消息。
- CLI（bcs-cli）子命令。