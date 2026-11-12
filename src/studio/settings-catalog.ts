import type {GameTab} from './game-tabs';

export type SettingTarget = {path: string; label: string; tab: GameTab; section: string; keywords?: string};
export type SettingRequest = SettingTarget & {serial: number};
const group = (tab: GameTab, section: string, prefix: string, fields: [string, string, string?][]): SettingTarget[] =>
  fields.map(([key, label, keywords])=>({path:prefix + key, label, tab, section, keywords}));

export const settingsCatalog: SettingTarget[] = [
  ...group('motion','motion-main','gyro.',[
    ['enabled','启用体感'], ['activation','开启按键'], ['activation_mode','开启方式'], ['pause_input','按住暂停'],
    ['activation_threshold','扳机与摇杆阈值'], ['sensitivity','体感灵敏度'], ['vertical_ratio','垂直灵敏度比例'],
  ]),
  ...group('motion','motion-direction','gyro.',[['invert_y','反转垂直方向']]),
  ...group('motion','motion-direction','gyro.motion.',[
    ['space','体感坐标'], ['invert_x','反转水平方向'], ['roll_contribution','横滚参与水平转向'], ['hold_angles','习惯握持角度'],
  ]),
  ...group('motion','motion-posture','gyro.motion.',[
    ['mode','控制方式'], ['reset_input','姿态归零按键'], ['neutral_input','设置正前方按键'],
    ['steering_range','达到满杆的转角'], ['recenter_strength','自动回中强度'], ['reset_ease_ms','手动归零过渡时间'], ['steering_exclude_y','只控制水平轴'],
    ['rotation.stick','旋转哪个摇杆'], ['rotation.sensitivity','旋转倍率'], ['rotation.deadzone','旋转死区'], ['rotation.smooth_ms','拇指方向稳定时间'],
    ['rotation.relative','相对模式：逐渐返回拇指方向'], ['rotation.return_ms','回归时间'], ['rotation.takeover','手腕转动时接管瞄准方向'],
    ['rotation.takeover_dps','体感接管速度'], ['rotation.outer','外圈交还拇指阈值'],
  ]),
  ...group('motion','motion-inertia','gyro.motion.trackball.',[
    ['horizontal_input','保持水平惯性'], ['vertical_input','保持垂直惯性'], ['decay','惯性衰减'],
  ]),
  ...group('motion','motion-edge','gyro.motion.edge.',[
    ['mode','边缘转向方式'], ['zone','开始持续转向的角度'], ['blend','接管过渡区间'], ['ramp','达到最大速度的区间'],
    ['max_dps','最大持续转速'], ['momentum','出界速度保留倍率'], ['decay_s','惯性衰减时间'], ['gyro_keep','边缘处保留手腕控制'], ['horizontal_only','只作用于水平方向'],
  ]),
  ...group('motion','motion-acceleration','gyro.acceleration.',[
    ['fast_multiplier','快速转动倍率'], ['start_dps','加速起点'], ['full_dps','达到最高倍率'], ['curve','加速度曲线'],
    ['natural_half','达到半程倍率的速度'], ['power_reference','参考转速'], ['power_exponent','幂指数'], ['sigmoid_mid','过渡中点'],
    ['sigmoid_width','过渡宽度'], ['jump_width','跳变过渡宽度'], ['points','自定义控制点'],
  ]),
  ...group('motion','motion-smoothing','gyro.',[['smoothing_ms','平滑时间']]),
  ...group('motion','motion-smoothing','gyro.motion.',[
    ['smoothing','平滑方式','One Euro 自适应滤波'], ['decay_threshold','退出平滑的转速'], ['one_euro_cutoff','最低截止频率'], ['one_euro_beta','速度响应系数'],
  ]),
  ...group('motion','motion-idle','gyro.',[['noise_threshold','静止抑制']]),
  ...group('motion','motion-idle','gyro.motion.',[['noise_gate','实测噪声抑制宽度'], ['cutoff_dps','低速截止'], ['recovery_ms','截止后的恢复时间']]),
  ...group('motion','motion-brake','gyro.motion.',[['brake_strength','减速刹车强度'], ['brake_threshold','刹车触发减速量']]),
  ...group('motion','motion-direction-stability','gyro.motion.',[['angle_snap','水平／垂直吸附角'], ['angle_snap_ease','柔和进入角度吸附'], ['drift_correction','使用重力修正姿态漂移']]),
  ...group('motion','motion-filters','gyro.',[['timestamp_source','采样时钟'], ['motion.custom_filters','自定义滤波']]),
  ...group('motion','motion-calibration','gyro.',[
    ['output','体感与 Flick 输出'], ['stick_full_scale_dps','游戏满杆角速度','满杆角速度 满杆速度 转角校准'], ['counts_per_degree','基础鼠标计数','转角校准'],
  ]),
  ...group('sticks','stick-input','input.',[['deadzone_left','左摇杆输入死区'], ['deadzone_right','右摇杆输入死区']]),
  ...group('sticks','stick-directions','input.',[['lstick_directions','左摇杆方向判定'], ['rstick_directions','右摇杆方向判定']]),
  ...group('sticks','stick-calibration','input.',[['lstick_center','左摇杆中心偏移'], ['rstick_center','右摇杆中心偏移'], ['lstick_radial','左摇杆圆周倍率'], ['rstick_radial','右摇杆圆周倍率']]),
  ...group('sticks','stick-zones','stick_zones.',[['left_outer','左摇杆外圈阈值'], ['right_outer','右摇杆外圈阈值'], ['hysteresis','区域切换回差']]),
  ...group('sticks','stick-input','', [['stick_rebound_ms','防回弹窗口','启用摇杆防回弹']]),
  ...group('sticks','stick-mouse','stick_mouse.',[['enabled','启用摇杆鼠标'], ['stick','摇杆鼠标使用的摇杆'], ['speed','最大移动速度'], ['exponent','摇杆鼠标响应']]),
  ...group('sticks','flick-main','gyro.motion.flick.',[
    ['enabled','启用 Flick Stick'], ['smooth_ms','转角释放时间','转身完成时间'], ['release_ease','转角缓出强度'], ['stick','Flick 使用的摇杆'], ['deadzone','快推转身阈值'],
  ]),
  ...group('sticks','flick-shared','gyro.motion.flick.',[
    ['standalone','关闭体感时也启用'],
    ['enable_input','按住启用（不设置则常开）'], ['suppress','原始摇杆输出'],
  ]),
  ...group('sticks','flick-tracking','gyro.motion.flick.',[['release_ratio','追踪下限比例'], ['disengage_ms','Flick 退出延时'], ['stabilise_ms','绕圈方向稳定']]),
  ...group('sticks','flick-trigger','gyro.motion.flick.',[['forward_deadzone','正前方死角'], ['speed_gate_ms','快速推杆判定时间'], ['slow_lock','慢推锁定为连续瞄准']]),
  ...group('sticks','stick-aim','gyro.motion.flick.',[['stick_aim','启用配套摇杆瞄准'], ['aim_sensitivity','摇杆瞄准倍率'], ['max_dps','连续瞄准基础转速'], ['vertical_ratio','摇杆纵向比例']]),
  ...group('triggers','trigger-stages','trigger_stages.',[
    ['left_mode','左扳机全拉模式'], ['right_mode','右扳机全拉模式'], ['soft_threshold','轻拉阈值','随扳机移动触发轻拉'], ['skip_delay_ms','快速全拉时间窗'],
  ]),
  ...group('triggers','trigger-input','input.',[
    ['ltrig_min','左扳机最小原始值'], ['ltrig_max','左扳机最大原始值'], ['rtrig_min','右扳机最小原始值'], ['rtrig_max','右扳机最大原始值'],
  ]),
  ...group('triggers','trigger-digital','input.',[
    ['digital_triggers','使用数字扳机'], ['ltrig_digital_threshold','左扳机数字阈值'], ['rtrig_digital_threshold','右扳机数字阈值'], ['trig_hysteresis','扳机数字回差'], ['trig_debounce_ms','扳机松开防抖时间'],
  ]),
  ...group('touch','touch-general','touchpad.',[['enabled','启用触摸板鼠标'], ['pointer_speed','触摸板指针速度'], ['scroll_speed','触摸板滚动速度'], ['natural_scroll','自然滚动']]),
];

export const settingForLabel = (label: string) => settingsCatalog.find(setting=>setting.label === label);
