import type {Status} from '../types';
import {inputLabel,outputLabel} from '../types';
import {t} from '../i18n';
import {triggerNames} from './BindingFields';
import {gamepadControlLabel} from './GamepadBindingEditor';
import styles from './MappingStatusPanel.module.css';

const reasons:Record<string,string>={
  idle:'未触发',active:'正在输出',latched:'输出保持中',blocked:'等待松开后重新触发',
  waiting_chord:'等待组合按键',waiting_long:'等待长按判定',waiting_double:'等待双击判定',
  waiting_release:'等待松开判定',claimed:'本次按压由其他触发方式占用',consumed:'组合键已优先触发',
};
export function MappingStatusPanel({status,profileId}:{status?:Status;profileId?:string}) {
  const running=!!status&&status.profile_id===profileId&&status.emitting&&!status.binding_capture&&!status.ui_navigation;
  const entries=running?(status.mapping??[]):[];
  const relevant=entries.filter(row=>row.reason!=='idle');
  return <details className={styles.panel} data-mapping-status>
    <summary>实时映射状态{running&&<span>{relevant.length}</span>}</summary>
    {!running?<p>此配置尚未输出，或正在录入／导航；恢复映射后可查看判定状态。</p>:<>
      <p>运行层：<span translate="no">{status.active_layers?.length?status.active_layers.join(' + '):t('基础层')}</span></p>
      <p>显示当前已保存配置的按键判定；短促状态可能在两次界面刷新之间完成。</p>
      {relevant.length?<ul>{relevant.map((row,index)=><li key={index}>
        <strong translate="no">{row.output.startsWith('pad:')?gamepadControlLabel(row.name.slice(4),status.output_gamepad?.target):row.name}</strong>
        <span>{inputLabel(row.input)} · {t(triggerNames[row.trigger])}{!row.output.startsWith('pad:')&&<> → {outputLabel(row.output)}</>}</span>
        <em data-active={row.active||row.latched}>{t(reasons[row.reason]??'未触发')}</em>
      </li>)}</ul>:<p>当前没有触发中的按键动作。</p>}
    </>}
  </details>;
}
