import {Range,Toggle} from './Controls';
import {OptionPicker} from './OptionPicker';
import {FlickStickSettings,type MotionUpdate} from './AdvancedMotion';
import {StickInputSettings,StickDirectionSettings,StickCalibrationSettings} from './ControllerInputSettings';
import {freezeInputDeadzones} from '../controller-input';
import type {Profile} from '../types';
import {SettingField,SettingSection,settingStyles} from './SettingLayout';
import styles from './MotionControls.module.css';

const stickChoices=[{options:[{value:'left',label:'左摇杆'},{value:'right',label:'右摇杆'}]}];

export function StickSettings({profile,update}:{profile:Profile;update:MotionUpdate}) {
  const stick=profile.stick_mouse;
  const zones=profile.stick_zones??{left_outer:0.85,right_outer:0.85,hysteresis:0.05};
  return <div className={settingStyles.page}>
    <StickInputSettings profile={profile} update={update}/>
    <FlickStickSettings profile={profile} update={update}/>
    <SettingSection id="stick-mouse" title="摇杆鼠标" summary={stick.enabled?<>{stick.speed} /s</>:'关闭'}>
      <Toggle label="启用摇杆鼠标" value={stick.enabled}
        onChange={value=>update(draft=>{freezeInputDeadzones(draft);draft.stick_mouse.enabled=value;})}/>
      {stick.enabled&&<>
        <SettingField label="摇杆鼠标使用的摇杆"><div className="studio-field">摇杆鼠标使用的摇杆
          <OptionPicker label="摇杆鼠标使用的摇杆" value={stick.stick} groups={stickChoices}
            onChange={value=>update(draft=>{freezeInputDeadzones(draft);draft.stick_mouse.stick=value;})}/>
        </div></SettingField>
        <Range label="最大移动速度" value={stick.speed} min={100} max={4000} step={100} unit=" /s"
          onChange={value=>update(draft=>{draft.stick_mouse.speed=value;})}/>
        <Range label="摇杆鼠标响应" value={stick.exponent} min={1} max={4} step={0.1}
          onChange={value=>update(draft=>{draft.stick_mouse.exponent=value;})} help="1 为线性。数值越大，小幅拨动时移动越慢。"/>
      </>}
    </SettingSection>
    <StickDirectionSettings profile={profile} update={update}/>
    <SettingSection id="stick-zones" title="摇杆内外圈映射" summary={<>左 {Math.round(zones.left_outer*100)}% · 右 {Math.round(zones.right_outer*100)}%</>}>
      <Range label="左摇杆外圈阈值" value={zones.left_outer} min={0.1} max={1} step={0.01} onChange={value=>update(p=>{p.stick_zones={...zones,left_outer:value};})}/>
      <Range label="右摇杆外圈阈值" value={zones.right_outer} min={0.1} max={1} step={0.01} onChange={value=>update(p=>{p.stick_zones={...zones,right_outer:value};})}/>
      <Range label="区域切换回差" value={zones.hysteresis} min={0} max={0.09} step={0.01} onChange={value=>update(p=>{p.stick_zones={...zones,hysteresis:value};})}/>
      <p className={styles.hint}>外圈按推杆幅度判断，不限方向，回到内圈后释放。内圈不包含中心死区。在「动作映射」中可将内圈或外圈绑定到按键、组合键或滚轮。</p>
    </SettingSection>
    <StickCalibrationSettings profile={profile} update={update}/>
  </div>;
}
