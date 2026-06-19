# 主角很帅插件 (Handsome Protagonist Skill)

## 功能说明
这个插件会在每次生成小说内容前，自动向LLM的prompt中注入"主角很帅"的描述，确保生成的文本中始终包含对主角外貌的正面描述。

## 使用方法
1. 插件默认处于启用状态，每次生成都会自动注入"主角很帅"
2. 如果需要临时禁用此功能，可以通过LLM工具调用 `toggle_handsome_protagonist` 函数
3. 状态会持久化保存，下次启动时保持上次的设置

## 技术实现
- 使用 `on_before_scene_write` 钩子在LLM生成前修改prompt
- 通过 `<protagonist_appearance>` 标签包裹注入内容，便于识别
- 提供工具接口让LLM可以控制是否启用此功能
- 状态数据安全存储在 `.novel/skills_data/handsome_state.json` 中

## 注意事项
- 此插件会持续影响所有场景生成，确保主角形象的一致性
- 可以通过工具随时开关，灵活控制使用场景