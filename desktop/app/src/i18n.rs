//! UI-only localization. Core command names and MCP wire keys never depend on language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    Zh,
    En,
}
impl Default for Locale {
    fn default() -> Self {
        if let Ok(value) = std::env::var("MOTION_LOCALE") {
            if let Some(locale) = Self::parse(&value) {
                return locale;
            }
        }
        #[cfg(target_os = "windows")]
        {
            #[link(name = "kernel32")]
            extern "system" {
                fn GetUserDefaultUILanguage() -> u16;
            }
            // Reads a process-independent Windows language identifier.
            if unsafe { GetUserDefaultUILanguage() } & 0x3ff == 0x04 {
                return Self::Zh;
            }
        }
        #[cfg(not(target_os = "windows"))]
        if std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_MESSAGES"))
            .or_else(|_| std::env::var("LANG"))
            .is_ok_and(|v| v.to_ascii_lowercase().starts_with("zh"))
        {
            return Self::Zh;
        }
        Self::En
    }
}
#[derive(Clone, Copy)]
pub enum Text {
    File,
    Edit,
    Composition,
    Layer,
    Effect,
    Animation,
    View,
    Window,
    Help,
    Project,
    EffectControls,
    Timeline,
    Save,
    Undo,
    Redo,
    NewSolid,
    NewProject,
    OpenProject,
    ExportPackage,
    Duplicate,
    Delete,
    Enable3D,
    Transform,
    Position,
    Rotation,
    Scale,
    Opacity,
    Effects,
    SelectLayer,
    NoEffects,
    Duration,
    Search,
    SourceName,
    Type,
    ParentLink,
    None,
    Play,
    Pause,
    FirstFrame,
    LastFrame,
    Fit,
    Auto,
    Full,
    Smooth,
    Economy,
    Ready,
    Saved,
    ProjectOpened,
    NewComposition,
    ProjectPackage,
    EnterValid,
    EnterFinite,
    SharedProject,
    Dock,
    Default,
    Language,
    Chinese,
    English,
    About,
    AboutDescription,
    Untitled,
    Solid,
    Items,
    Layers,
    Fps,
    AddPositionKey,
    ShowComposition,
    ShowEffects,
    ResetWorkspace,
    FrameStep,
    OperationFailed,
}
impl Locale {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().split(['-', '_']).next()? {
            "zh" => Some(Self::Zh),
            "en" => Some(Self::En),
            _ => None,
        }
    }
    pub fn t(self, key: Text) -> &'static str {
        use Text::*;
        let (zh, en) = match key {
            File => ("文件", "File"),
            Edit => ("编辑", "Edit"),
            Composition => ("合成", "Composition"),
            Layer => ("图层", "Layer"),
            Effect => ("效果", "Effect"),
            Animation => ("动画", "Animation"),
            View => ("视图", "View"),
            Window => ("窗口", "Window"),
            Help => ("帮助", "Help"),
            Project => ("项目", "Project"),
            EffectControls => ("效果控件", "Effect Controls"),
            Timeline => ("时间轴", "Timeline"),
            Save => ("保存", "Save"),
            Undo => ("撤销", "Undo"),
            Redo => ("重做", "Redo"),
            NewSolid => ("新建纯色", "New Solid"),
            NewProject => ("新建工程", "New Project"),
            OpenProject => ("打开工程", "Open Project"),
            ExportPackage => ("导出工程包", "Export Project Package"),
            Duplicate => ("复制图层", "Duplicate"),
            Delete => ("删除", "Delete"),
            Enable3D => ("启用 3D", "Enable 3D"),
            Transform => ("变换", "Transform"),
            Position => ("位置", "Position"),
            Rotation => ("旋转", "Rotation"),
            Scale => ("缩放", "Scale"),
            Opacity => ("不透明度", "Opacity"),
            Effects => ("效果", "Effects"),
            SelectLayer => ("在时间轴中选择一个图层", "Select a layer in the timeline"),
            NoEffects => ("未添加效果", "No effects applied"),
            Duration => ("时长", "Duration"),
            Search => ("搜索项目", "Search project"),
            SourceName => ("源名称", "Source Name"),
            Type => ("类型", "Type"),
            ParentLink => ("父级与链接", "Parent & Link"),
            None => ("无", "None"),
            Play => ("播放", "Play"),
            Pause => ("暂停", "Pause"),
            FirstFrame => ("第一帧", "First Frame"),
            LastFrame => ("最后一帧", "Last Frame"),
            Fit => ("适应", "Fit"),
            Auto => ("自动", "Auto"),
            Full => ("完整", "Full"),
            Smooth => ("流畅", "Smooth"),
            Economy => ("节能", "Economy"),
            Ready => ("就绪", "Ready"),
            Saved => ("已保存", "Saved"),
            ProjectOpened => ("工程已打开", "Project opened"),
            NewComposition => ("已新建 1920 × 1080 合成", "New 1920 × 1080 composition"),
            ProjectPackage => ("工程包", "Project package"),
            EnterValid => (
                "请输入有效数值，Esc 取消",
                "Enter a valid number; Esc cancels",
            ),
            EnterFinite => ("请输入有限数值", "Enter a finite number"),
            SharedProject => ("共享工程与时间轴", "Shared project / timeline"),
            Dock => ("停靠", "Dock"),
            Default => ("默认", "Default"),
            Language => ("语言", "Language"),
            Chinese => ("简体中文", "简体中文"),
            English => ("English", "English"),
            About => ("关于 Motion Studio", "About Motion Studio"),
            AboutDescription => (
                "Motion Studio · AI 自主驱动的创作工具实验",
                "Motion Studio · An AI-driven creative tool experiment",
            ),
            Untitled => ("未命名工程", "Untitled Project"),
            Solid => ("纯色", "Solid"),
            Items => ("项目", "items"),
            Layers => ("图层", "layers"),
            Fps => ("帧/秒", "fps"),
            AddPositionKey => ("启用位置关键帧", "Enable Position Keyframes"),
            ShowComposition => ("合成面板", "Composition Panel"),
            ShowEffects => ("效果控件面板", "Effect Controls Panel"),
            ResetWorkspace => ("重置默认工作区", "Reset Default Workspace"),
            FrameStep => ("方向键：逐帧", "Arrows: frame step"),
            OperationFailed => ("操作失败", "Operation failed"),
        };
        if self == Self::Zh {
            zh
        } else {
            en
        }
    }
    pub fn panel(self, id: aem_ui::dock::DockId) -> &'static str {
        self.t(match id.0 {
            1 => Text::Project,
            2 => Text::EffectControls,
            3 => Text::Composition,
            4 => Text::Timeline,
            _ => Text::Window,
        })
    }
    pub fn error(self, error: &str) -> String {
        if self == Self::Zh && error.starts_with("project is already open") {
            return "工程已被另一个进程打开，请关闭后再试。".into();
        }
        format!("{}: {error}", self.t(Text::OperationFailed))
    }
}
