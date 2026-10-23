# 图标来源

全部来自 JetBrains/intellij-community（Apache License 2.0，见同目录 LICENSE-Apache-2.0.txt 与 NOTICE.txt）。
取的是深色主题版（`*_dark.svg`；没有深色版的取原版），文件内容未改动，只是按 lite-ide 的名字重命名。

| lite-ide | 上游路径 |
|---|---|
| `icons/pull.svg` | `platform/icons/src/expui/vcs/update_dark.svg` |
| `icons/push.svg` | `platform/icons/src/expui/vcs/push_dark.svg` |
| `icons/commit.svg` | `platform/icons/src/expui/vcs/commit_dark.svg` |
| `icons/folder.svg` | `platform/icons/src/expui/nodes/folder_dark.svg` |
| `icons/to-xml.svg` | `platform/icons/src/expui/gutter/implementingMethod_dark.svg`（MyBatis：接口方法旁边，点了跳到 XML 里那段 SQL —— IDEA 用它表示「这个方法有实现，跳过去」） |
| `icons/to-java.svg` | `platform/icons/src/expui/gutter/implementedMethod_dark.svg`（MyBatis：XML 语句旁边，点了跳回接口方法 —— IDEA 用它表示「跳回声明」） |
| `icons/bell.svg` | `platform/icons/src/expui/toolwindows/notifications_dark.svg`（2026-10-23 换掉自绘的暂代版） |
| `icons/structure.svg` | `platform/icons/src/expui/toolwindows/structure_dark.svg`（结构工具窗原来暂用 `compact` 那个「按包分组」） |
| `icons/bookmarks.svg` | `platform/icons/src/expui/toolwindows/bookmarks_dark.svg` |
| `icons/bookmark.svg` | `platform/icons/src/expui/gutter/bookmark_dark.svg`（14×14，黄色，行号旁的书签） |
| `icons/tag.svg` | **lite-ide 自绘**：IDEA 新 UI 没有 git 标签的独立图标（提交历史里的标签是代码画的色块，`nodes/tag` 是 XML 标签 `<>`）。描边按新 UI 的 1px 画 |
| `icons/maven.svg` | `plugins/maven/src/main/resources/images/expui/toolwindow/maven_dark.svg`（Maven 插件的工具窗图标，不在 platform/icons 里） |
| `icons/maven-project.svg` | `plugins/maven/src/main/resources/images/expui/build/mavenProject_dark.svg`（Maven 工具窗里的项目 / 模块节点（IDEA 的 `MavenIcons.MavenProject`）） |
| `icons/maven-plugin.svg` | `plugins/maven/src/main/resources/images/expui/build/mavenPlugin_dark.svg`（Plugins 底下的一个插件） |
| `icons/maven-goal.svg` | `plugins/maven/src/main/resources/images/expui/build/mavenPluginGoal_dark.svg`（插件的目标） |
| `icons/maven-profiles.svg` | `plugins/maven/src/main/resources/images/expui/build/mavenProfiles_dark.svg`（Profiles 节点） |
| `icons/lifecycle.svg` | `platform/icons/src/expui/build/taskGroup_dark.svg`（Lifecycle / Plugins 节点（IDEA 的 `AllIcons.Nodes.ConfigFolder`，新 UI 映射到这个）） |
| `icons/phase.svg` | `platform/external-system-impl/resources/icons/newui/task_dark.svg`（生命周期阶段（IDEA 的 `ExternalSystemIcons.Task`）） |
| `icons/library-folder.svg` | `platform/icons/src/expui/nodes/libraryFolder_dark.svg`（Dependencies 节点（`AllIcons.Nodes.PpLibFolder`）） |
| `icons/library.svg` | `platform/icons/src/expui/nodes/library_dark.svg`（一个依赖（`AllIcons.Nodes.PpLib`）） |
| `icons/run-anything.svg` | `platform/icons/src/expui/general/runAnything_dark.svg`（执行 Maven 目标（`Maven.ExecuteGoal`）） |
| `icons/offline.svg` | `platform/icons/src/expui/build/toggleOfflineMode_dark.svg`（离线模式开关（`Maven.ToggleOffline`）） |
| `icons/skip-tests.svg` | `platform/icons/src/expui/run/showIgnored_dark.svg`（跳过测试开关（`Maven.ToggleSkipTests`）） |
| `icons/expand.svg` | `platform/icons/src/expui/general/expandAll_dark.svg`（全部展开） |
| `icons/services.svg` | `platform/icons/src/expui/toolwindows/services_dark.svg`（Services 工具窗，2026-10-19） |
| `icons/error.svg` | `platform/icons/src/expui/status/error_dark.svg`（Services 里的 Failed 分组） |
| `icons/debug.svg` | `platform/icons/src/expui/run/debug_dark.svg`（调试启动，docs/IDEA.md 第 18 节，2026-10-20） |
| `icons/resume.svg` | `platform/icons/src/expui/run/resume_dark.svg`（继续 F9） |
| `icons/pause.svg` | `platform/icons/src/expui/run/pause_dark.svg`（暂停） |
| `icons/step-over.svg` | `platform/icons/src/expui/run/stepOver_dark.svg`（单步跳过 F8） |
| `icons/step-into.svg` | `platform/icons/src/expui/run/stepInto_dark.svg`（单步进入 F7） |
| `icons/step-out.svg` | `platform/icons/src/expui/run/stepOut_dark.svg`（单步跳出 ⇧F8） |
| `icons/breakpoint.svg` | `platform/icons/src/expui/breakpoints/breakpoint_dark.svg`（行号上的断点；14×14，编辑器行号栏用 CSS 背景画，不走 `Icon`） |
| `icons/sidebar.svg` | `platform/icons/src/expui/general/layout_dark.svg` |
| `icons/files.svg` | `platform/icons/src/expui/toolwindows/project_dark.svg` |
| `icons/git.svg` | `platform/icons/src/expui/toolwindows/vcs_dark.svg` |
| `icons/search.svg` | `platform/icons/src/expui/general/search_dark.svg` |
| `icons/history.svg` | `platform/icons/src/expui/general/history_dark.svg` |
| `icons/note.svg` | `platform/icons/src/expui/fileTypes/scratches_dark.svg`（IDEA「Scratches and Consoles」的图标） |
| `icons/locate.svg` | `platform/icons/src/expui/general/locate_dark.svg` |
| `icons/collapse.svg` | `platform/icons/src/expui/general/collapseAll_dark.svg` |
| `icons/follow.svg` | `platform/icons/src/expui/general/autoscrollFromSource_dark.svg` |
| `icons/compact.svg` | `platform/icons/src/expui/actions/groupByPackage_dark.svg` |
| `icons/pin.svg` | `platform/icons/src/expui/general/pin_dark.svg` |
| `icons/more-v.svg` | `platform/icons/src/expui/general/moreVertical_dark.svg` |
| `icons/minus.svg` | `platform/icons/src/expui/general/hide_dark.svg` |
| `icons/refresh.svg` | `platform/icons/src/expui/general/refresh_dark.svg` |
| `icons/check.svg` | `platform/icons/src/expui/actions/checked_dark.svg` |
| `icons/plus.svg` | `platform/icons/src/expui/general/add_dark.svg` |
| `icons/warn.svg` | `platform/icons/src/expui/status/warning_dark.svg` |
| `icons/chevron-up.svg` | `platform/icons/src/expui/general/chevronUp_dark.svg` |
| `icons/chevron-down.svg` | `platform/icons/src/expui/general/chevronDown_dark.svg` |
| `icons/chevron-right.svg` | `platform/icons/src/expui/general/chevronRight_dark.svg` |
| `icons/swap.svg` | `platform/icons/src/expui/actions/swapPanels_dark.svg` |
| `icons/more-h.svg` | `platform/icons/src/expui/general/moreHorizontal_dark.svg` |
| `icons/branch-current.svg` | `platform/icons/src/expui/general/vcs_dark.svg` |
| `icons/remote.svg` | `platform/icons/src/expui/toolwindows/web_dark.svg` |
| `icons/x.svg` | `platform/icons/src/expui/general/closeSmall_dark.svg` |
| `icons/undo.svg` | `platform/icons/src/expui/vcs/revert_dark.svg` |
| `icons/match-case.svg` | `platform/icons/src/expui/inline/matchCase_dark.svg` |
| `icons/whole-word.svg` | `platform/icons/src/expui/inline/exactWords_dark.svg` |
| `icons/regex.svg` | `platform/icons/src/expui/inline/regex_dark.svg` |
| `icons/file/folder.svg` | `platform/icons/src/expui/nodes/folder_dark.svg` |
| `icons/file/package.svg` | `platform/icons/src/expui/nodes/package_dark.svg` |
| `icons/file/sourceRoot.svg` | `platform/icons/src/expui/nodes/sourceRoot_dark.svg` |
| `icons/file/testRoot.svg` | `platform/icons/src/expui/nodes/testRoot_dark.svg` |
| `icons/file/resourcesRoot.svg` | `platform/icons/src/expui/nodes/resourcesRoot_dark.svg` |
| `icons/file/testResourcesRoot.svg` | `platform/icons/src/expui/nodes/testResourcesRoot_dark.svg` |
| `icons/file/excludeRoot.svg` | `platform/icons/src/expui/nodes/excludeRoot_dark.svg` |
| `icons/file/java.svg` | `platform/icons/src/expui/fileTypes/java_dark.svg` |
| `icons/file/javaScript.svg` | `platform/icons/src/expui/fileTypes/javaScript_dark.svg` |
| `icons/file/typeScript.svg` | `platform/icons/src/expui/fileTypes/typeScript_dark.svg` |
| `icons/file/vue.svg` | `platform/icons/src/expui/fileTypes/vue.svg` |
| `icons/file/json.svg` | `platform/icons/src/expui/fileTypes/json_dark.svg` |
| `icons/file/yaml.svg` | `platform/icons/src/expui/fileTypes/yaml_dark.svg` |
| `icons/file/xml.svg` | `platform/icons/src/expui/fileTypes/xml_dark.svg` |
| `icons/file/markdown.svg` | `platform/icons/src/expui/fileTypes/markdown_dark.svg` |
| `icons/file/properties.svg` | `platform/icons/src/expui/fileTypes/properties_dark.svg` |
| `icons/file/sql.svg` | `platform/icons/src/expui/fileTypes/sql_dark.svg` |
| `icons/file/shell.svg` | `platform/icons/src/expui/fileTypes/shell_dark.svg` |
| `icons/file/html.svg` | `platform/icons/src/expui/fileTypes/html_dark.svg` |
| `icons/file/css.svg` | `platform/icons/src/expui/fileTypes/css_dark.svg` |
| `icons/file/csv.svg` | `platform/icons/src/expui/fileTypes/csv_dark.svg` |
| `icons/file/text.svg` | `platform/icons/src/expui/fileTypes/text_dark.svg` |
| `icons/file/toml.svg` | `platform/icons/src/expui/fileTypes/toml_dark.svg` |
| `icons/file/gradle.svg` | `platform/icons/src/expui/fileTypes/gradle_dark.svg` |
| `icons/file/docker.svg` | `platform/icons/src/expui/fileTypes/docker_dark.svg` |
| `icons/file/gitignore.svg` | `platform/icons/src/expui/fileTypes/gitignore.svg` |
| `icons/file/editorConfig.svg` | `platform/icons/src/expui/fileTypes/editorConfig_dark.svg` |
| `icons/file/config.svg` | `platform/icons/src/expui/fileTypes/config_dark.svg` |
| `icons/file/image.svg` | `platform/icons/src/expui/fileTypes/image_dark.svg` |
| `icons/file/archive.svg` | `platform/icons/src/expui/fileTypes/archive_dark.svg` |
| `icons/file/c.svg` | `platform/icons/src/expui/fileTypes/c_dark.svg` |
| `icons/file/cpp.svg` | `platform/icons/src/expui/fileTypes/cpp_dark.svg` |
| `icons/file/h.svg` | `platform/icons/src/expui/fileTypes/h_dark.svg` |
| `icons/file/unknown.svg` | `platform/icons/src/expui/fileTypes/unknown_dark.svg` |
| `icons/terminal.svg` | `plugins/terminal/resources/icons/expui/toolwindow/terminal_dark.svg` |
| `icons/file/python.svg` | `python/python-parser/resources/icons/com/jetbrains/python/parser/expui/python_dark.svg` |
| `icons/file/kotlin.svg` | `plugins/kotlin/base/resources/resources/org/jetbrains/kotlin/idea/icons/expui/kotlin_dark.svg` |
| `icons/file/rust.svg` | `platform/icons/src/language/rust_dark.svg`（上游没有 expui 版，这一份已经是 16px 扁平风格） |
| `icons/file/go.svg` | `platform/icons/src/language/go_dark.svg`（同上） |
| `icons/run.svg` | `platform/icons/src/expui/run/run_dark.svg`（#48，2026-10-10） |
| `icons/stop.svg` | `platform/icons/src/expui/run/stop_dark.svg`（同上） |
| `icons/rerun.svg` | `platform/icons/src/expui/run/rerun_dark.svg`（同上） |
