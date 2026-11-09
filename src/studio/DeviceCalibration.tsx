import {useEffect, useState} from 'react';
import type {Device, SensorCalibration, Status} from '../types';
import {Range, SettingsGroup as Group, Toggle} from './Controls';
import motionStyles from './MotionControls.module.css';
import styles from './DeviceCalibration.module.css';

const defaultSensor=():SensorCalibration=>({accel_offset:[0,0,0],accel_scale:[1,1,1],gyro_sign:[1,1,1],accel_sign:[1,1,1],mounting:[1,0,0,0,1,0,0,0,1],gyro_noise_dps:0,accel_noise_g:0,accel_faces:[[null,null],[null,null],[null,null]]});
const axes = ['X', 'Y', 'Z'];

type Props = {
  device: Device;
  status?: Status;
  sensor?: SensorCalibration;
  canCalibrate: boolean;
  calibrating: boolean;
  blocked: boolean;
  onCalibrate: () => void;
  onMotionAction: (action: string, axis?: number, positive?: boolean) => void;
  onSaveSensor: (value: SensorCalibration) => void;
};

export function DeviceCalibration({device,status,sensor,canCalibrate,calibrating,blocked,onCalibrate,onMotionAction,onSaveSensor}:Props) {
  const [cal, setCal] = useState(sensor ?? defaultSensor());
  const [mount, setMount] = useState([0,0,0]);
  useEffect(()=>{
    const calibration = sensor ?? defaultSensor();
    setCal(calibration);
    const matrix = calibration.mounting;
    const pitch = Math.asin(Math.max(-1, Math.min(1, -matrix[6])));
    setMount([Math.atan2(matrix[7],matrix[8]), pitch, Math.atan2(matrix[3],matrix[0])]
      .map(radians=>radians*180/Math.PI));
  },[sensor]);
  return <div className={motionStyles.panel + ' ' + styles.body}>
    <p className={motionStyles.notice}>校准结果按设备保存在本机，同一手柄的游戏配置共用。</p>
    <fieldset className={styles.columns + ' studio-content'} disabled={blocked}>
      <div className={styles.column}>
        <Group title="陀螺仪静止校准">
          <p className={motionStyles.hint}>放稳手柄，2 秒后开始采样，约 5 秒完成。</p>
          <p className="studio-muted">校准成功后会自动保存陀螺仪零偏和噪声底。</p>
          <button type="button" className="secondary-btn" disabled={!canCalibrate || calibrating}
            onClick={onCalibrate}>静止校准与噪声测量</button>
          {!canCalibrate && !calibrating && <p className={motionStyles.hint}>先开始观察输入，再进行校准。</p>}
        </Group>
        <Group title="加速度计六面校准">
          <p className={motionStyles.hint}>依次将指定方向朝上并放稳，采集每个轴的正反两面。每个轴两面都记录后，会计算该轴的偏移与比例。X 指向右握把，Y 指向手柄正面，Z 指向身体。</p>
          <div className={styles.faceGrid}>
            {axes.flatMap((name, axis) => [false, true].map(positive => {
              const saved = sensor?.accel_faces[axis]?.[positive ? 1 : 0] != null;
              return <button className="secondary-btn" key={name + positive} type="button"
                disabled={!canCalibrate || !device.accelerometer} onClick={() => onMotionAction('calibrate_face', axis, positive)}>
                {positive ? '+' : '−'}{name} 朝上 {saved ? '✓' : ''}
              </button>;
            }))}
          </div>
        </Group>
        {calibrating && <progress aria-label="传感器校准进度" max={1} value={status?.calibration_progress ?? 0} />}
      </div>
      <div className={styles.column}>
        <Group title="设备方向与噪声">
          <div className={styles.noiseGrid}>
            <Range label="陀螺仪噪声底" value={cal.gyro_noise_dps} min={0} max={20} step={0.01} unit=" °/s"
              onChange={gyro_noise_dps=>setCal(c=>({...c,gyro_noise_dps}))}/>
            <Range label="加速度噪声底" value={cal.accel_noise_g} min={0} max={1} step={0.001} unit=" g"
              onChange={accel_noise_g=>setCal(c=>({...c,accel_noise_g}))}/>
          </div>
          <details><summary>传感器方向与加速度修正</summary>
            <p className={motionStyles.hint}>轴反转和安装方向用于修正传感器坐标。加速度计的偏移和比例由六面校准计算，也可在这里手动微调。</p>
            <div className={styles.sensorGrid}>
              {axes.map((name,i)=><div className={styles.axis} key={name}>
                <Toggle label={'反转陀螺仪 '+name} value={cal.gyro_sign[i]===-1} onChange={v=>setCal(c=>({...c,gyro_sign:c.gyro_sign.map((x,j)=>i===j?(v?-1:1):x)}))}/>
                <Toggle label={'反转加速度 '+name} value={cal.accel_sign[i]===-1} onChange={v=>setCal(c=>({...c,accel_sign:c.accel_sign.map((x,j)=>i===j?(v?-1:1):x)}))}/>
                <Range label={'加速度 '+name+' 偏移'} value={cal.accel_offset[i]} min={-5} max={5} step={0.01} unit=" m/s²" onChange={v=>setCal(c=>({...c,accel_offset:c.accel_offset.map((x,j)=>j===i?v:x)}))}/>
                <Range label={'加速度 '+name+' 比例'} value={cal.accel_scale[i]} min={0.5} max={2} step={0.001} onChange={v=>setCal(c=>({...c,accel_scale:c.accel_scale.map((x,j)=>j===i?v:x)}))}/>
              </div>)}
            </div>
            <div className={styles.sensorGrid}>
              {axes.map((name,i)=><Range key={name} label={'绕 '+name+' 轴修正安装方向'} value={mount[i]} min={-180} max={180} step={1} unit="°" onChange={v=>{
                const angles=mount.map((x,j)=>j===i?v:x);setMount(angles);
                const [x,y,z]=angles.map(a=>a*Math.PI/180),[cx,cy,cz]=[Math.cos(x),Math.cos(y),Math.cos(z)],[sx,sy,sz]=[Math.sin(x),Math.sin(y),Math.sin(z)];
                setCal(c=>({...c,mounting:[cz*cy,cz*sy*sx-sz*cx,cz*sy*cx+sz*sx,sz*cy,sz*sy*sx+cz*cx,sz*sy*cx-cz*sx,-sy,cy*sx,cy*cx]}));
              }}/>)}
            </div>
          </details>
          <div className="studio-actions"><button type="button" className="secondary-btn" onClick={()=>onSaveSensor(cal)}>保存设备校准</button>
            <button type="button" className="ghost-btn" onClick={()=>{const c=defaultSensor();setCal(c);setMount([0,0,0]);onSaveSensor(c);}}>恢复设备默认校准</button></div>
        </Group>
      </div>
    </fieldset>
  </div>;
}
