import { Range, Toggle } from './Controls';
import { OptionPicker } from './OptionPicker';
import { FlickStickSettings, type MotionUpdate } from './AdvancedMotion';
import { StickInputSettings } from './ControllerInputSettings';
import { freezeInputDeadzones } from '../controller-input';
import type { Profile } from '../types';
import styles from './MotionControls.module.css';

const stickChoices = [{options:[{value:'left',label:'左摇杆'}, {value:'right',label:'右摇杆'}]}];

export function StickSettings({ profile, update }: { profile: Profile; update: MotionUpdate }) {
  const stick = profile.stick_mouse;
  const zones=profile.stick_zones??{left_outer:0.85,right_outer:0.85,hysteresis:0.05};
  return <section className={styles.panel + ' app-card'}>
    <h2>摇杆设置</h2>
    <StickInputSettings profile={profile} update={update}/>
    <div className={styles.group}>
      <div className={styles.groupHeading}><h3>摇杆内外圈</h3></div>
      <Range label="左摇杆外圈阈值" value={zones.left_outer} min={0.1} max={1} step={0.01} onChange={value=>update(profile=>{profile.stick_zones={...zones,left_outer:value};})}/>
      <Range label="右摇杆外圈阈值" value={zones.right_outer} min={0.1} max={1} step={0.01} onChange={value=>update(profile=>{profile.stick_zones={...zones,right_outer:value};})}/>
      <Range label="区域切换回差" value={zones.hysteresis} min={0} max={0.09} step={0.01} onChange={value=>update(profile=>{profile.stick_zones={...zones,hysteresis:value};})}/>
      <p className={styles.hint}>外圈按推杆幅度判断，不限方向，回到内圈后释放。内圈不包含中心死区。在「动作映射」中可将内圈或外圈绑定到按键、组合键或滚轮。</p>
      <Toggle label="启用摇杆防回弹" value={profile.stick_rebound_ms!=null} onChange={enabled=>update(profile=>{profile.stick_rebound_ms=enabled?40:null;})}/>
      {profile.stick_rebound_ms!=null&&<Range label="防回弹窗口" value={profile.stick_rebound_ms} min={10} max={150} step={5} unit=" ms" onChange={ms=>update(profile=>{profile.stick_rebound_ms=ms;})}/>}
      <p className={styles.hint}>复用 FlexInput 的回弹过滤，抑制快速松杆时短暂越过中心造成的反向输入。默认关闭，可按手柄手感启用。</p>
    </div>
    <div className={styles.grid}>
      <div className={styles.group}>
        <div className={styles.groupHeading}><h3>摇杆鼠标</h3></div>
        <Toggle label="启用摇杆鼠标" value={stick.enabled} onChange={value => update(draft => { freezeInputDeadzones(draft); draft.stick_mouse.enabled = value; })} />
        <div className="studio-field">摇杆鼠标使用的摇杆
          <OptionPicker label="摇杆鼠标使用的摇杆" value={stick.stick} groups={stickChoices}
            onChange={value=>update(draft=>{freezeInputDeadzones(draft);draft.stick_mouse.stick=value;})}/>
        </div>
        <Range label="最大移动速度" value={stick.speed} min={100} max={4000} step={100} unit=" /s"
          onChange={value => update(draft => { draft.stick_mouse.speed = value; })} />
      </div>
      <div className={styles.group}>
        <div className={styles.groupHeading}><h3>摇杆鼠标响应</h3></div>
        <Range label="响应曲线" value={stick.exponent} min={1} max={4} step={0.1}
          onChange={value => update(draft => { draft.stick_mouse.exponent = value; })} help="1 为线性。数值越大，小幅拨动时移动越慢。" />
      </div>
    </div>
    <FlickStickSettings profile={profile} update={update}/>
  </section>;
}
