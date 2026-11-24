import {memo, useMemo} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {useLanguage} from '../i18n';
import {Range,Toggle} from './Controls';
import {OptionPicker} from './OptionPicker';
import {FlickStickSettings,GameStickResponse,MotionChoice,type MotionUpdate} from './AdvancedMotion';
import {bezierFromKnots} from '../bezier';
import {StickInputSettings,StickDirectionSettings,StickCalibrationSettings} from './ControllerInputSettings';
import {freezeInputDeadzones} from '../controller-input';
import type {Profile, ResponsePoint} from '../types';
import {CurveEditor} from './CurveEditor';
import {BezierCurveEditor} from './BezierCurveEditor';
import {useCurvePreview} from './useCurvePreview';
import {SettingField,SettingLabel,SettingSection,settingStyles} from './SettingLayout';
import styles from './MotionControls.module.css';

const stickChoices=[{options:[{value:'left',label:'左摇杆'},{value:'right',label:'右摇杆'}]}];

const linearResponse: ResponsePoint[] = [[0,0],[1,1]];
function StickMouseCurve({stick, update}: {stick: Profile['stick_mouse']; update: MotionUpdate}) {
  const custom = stick.response_points;
  const bezier = stick.response_model==='bezier';
  const key = !bezier && !custom && stick.exponent!==1 ? String(stick.exponent) : undefined;
  const reply = useCurvePreview(key,()=>invoke<ResponsePoint[]>('preview_stick_mouse_response',{exponent:stick.exponent}));
  const sampled = stick.exponent===1 ? linearResponse : reply?.key===key ? reply?.data : undefined;
  // Show the complete native table and a small editable selection of its knots.
  // Merely viewing the graph or selecting a handle keeps the exponent profile intact.
  const handles = useMemo(()=>sampled?.filter((_,i)=>i%16===0 || i===sampled.length-1),[sampled]);
  const points = custom??handles;
  const paths=['stick_mouse.response_model','stick_mouse.response_bezier'];
  return <SettingField label="摇杆鼠标响应曲线" path={[...paths,'stick_mouse.response_points']}>
    <MotionChoice label="曲线类型" settingPath={[]} value={bezier?'bezier':'points'} options={[
      ['points',custom?'分段曲线':'指数曲线'],['bezier','贝塞尔曲线'],
    ]} onChange={model=>update(p=>{
      const s=p.stick_mouse;
      if (model==='bezier' && !s.response_bezier?.length)
        s.response_bezier=bezierFromKnots(points??[[0,0,s.exponent],[1,1]]);
      s.response_model=model as typeof s.response_model;
    })}/>
    {bezier && stick.response_bezier?.length ? <BezierCurveEditor domain="stick_mouse" points={stick.response_bezier}
      onChange={next=>update(p=>{p.stick_mouse.response_bezier=next;})}/> : <>
    {points && <CurveEditor kind="stick" points={points} sampled={custom?undefined:sampled}
      onChange={next=>update(p=>{p.stick_mouse.response_points=next;})}/>}
    {custom ? <button type="button" className="ghost-btn" onClick={()=>update(p=>{delete p.stick_mouse.response_points;})}>恢复指数曲线</button>
      : points ? <p className={styles.hint}>首次编辑控制点时切换为分段响应，可随时恢复原指数。</p>
      : <p className={styles.hint} role="status">{reply?.key===key?'曲线暂未更新，请检查当前参数。':'正在计算响应曲线…'}</p>}</>}
  </SettingField>;
}

export const StickSettings = memo(function StickSettings({profile,update}:{profile:Profile;update:MotionUpdate}) {
  useLanguage();
  const stick=profile.stick_mouse;
  const zones=profile.stick_zones??{left_outer:0.85,right_outer:0.85,hysteresis:0.05};
  return <div className={settingStyles.page}>
    <StickInputSettings profile={profile} update={update}/>
    <SettingSection id="stick-game-response" title="虚拟右摇杆响应" lazy>
      <Toggle label="矫正实体摇杆输出" value={profile.gyro.right_stick_curve?.linearize_stick??false}
        onChange={value=>update(p=>{p.gyro.right_stick_curve={...p.gyro.right_stick_curve,linearize_stick:value};})}/>
      <p className={styles.hint}>将实体摇杆映射到虚拟右摇杆后，有效推杆量 50% 对应满杆转速的 50%。可以单独使用，也可以与体感和快速转向共用这条游戏曲线。</p>
      <GameStickResponse profile={profile} update={update}/>
    </SettingSection>
    <FlickStickSettings profile={profile} update={update}/>
    <SettingSection id="stick-mouse" title="摇杆鼠标" summary={stick.enabled?<>{stick.speed} 计数/s</>:'关闭'}>
      <Toggle label="启用摇杆鼠标" value={stick.enabled}
        onChange={value=>update(draft=>{freezeInputDeadzones(draft);draft.stick_mouse.enabled=value;})}/>
      {stick.enabled&&<>
        <SettingField label="摇杆鼠标使用的摇杆"><div className="studio-field"><SettingLabel label="摇杆鼠标使用的摇杆"/>
          <OptionPicker label="摇杆鼠标使用的摇杆" value={stick.stick} groups={stickChoices}
            onChange={value=>update(draft=>{freezeInputDeadzones(draft);draft.stick_mouse.stick=value;})}/>
        </div></SettingField>
        <Range label="最大移动速度" value={stick.speed} min={100} max={10000} step={100} unit=" 计数/s"
          onChange={value=>update(draft=>{draft.stick_mouse.speed=value;})}/>
        {!stick.response_points && stick.response_model!=='bezier' && <Range label="摇杆鼠标响应" value={stick.exponent} min={1} max={4} step={0.1}
          onChange={value=>update(draft=>{draft.stick_mouse.exponent=value;})} help="1 为线性。数值越大，小幅拨动时移动越慢。"/>}
        <StickMouseCurve stick={stick} update={update}/>
      </>}
    </SettingSection>
    <StickDirectionSettings profile={profile} update={update}/>
    <SettingSection id="stick-zones" title="摇杆内外圈映射" summary={<>左 {Math.round(zones.left_outer*100)}% · 右 {Math.round(zones.right_outer*100)}%</>}>
      <Range label="左摇杆外圈阈值" value={zones.left_outer} min={0.1} max={1} step={0.01} onChange={value=>update(p=>{p.stick_zones={...zones,left_outer:value};})}/>
      <Range label="右摇杆外圈阈值" value={zones.right_outer} min={0.1} max={1} step={0.01} onChange={value=>update(p=>{p.stick_zones={...zones,right_outer:value};})}/>
      <Range label="区域切换回差" value={zones.hysteresis} min={0} max={0.09} step={0.01} onChange={value=>update(p=>{p.stick_zones={...zones,hysteresis:value};})}/>
      <p className={styles.hint}>在「动作映射」中可绑定整个外圈，也可分别绑定上、下、左、右外圈。四个方向各占 90°，沿外圈转动即可切换方向。整圈和四向共用这里的阈值与回差。内圈从中心死区之外开始。</p>
    </SettingSection>
    <StickCalibrationSettings profile={profile} update={update}/>
  </div>;
});
