import type {Profile, TriggerStageMode, TriggerStagesConfig} from '../types';
import {Range, Toggle} from './Controls';
import {SettingField,SettingSection,settingStyles} from './SettingLayout';
import {OptionPicker} from './OptionPicker';
import type {MotionUpdate} from './AdvancedMotion';
import {TriggerInputSettings} from './ControllerInputSettings';
import styles from './MotionControls.module.css';

const triggerModes: Record<TriggerStageMode, string> = {
  no_full: '仅轻拉',
  no_skip: '全拉时保留轻拉',
  no_skip_exclusive: '全拉时替换轻拉',
  may_skip: '快速全拉跳过轻拉，慢拉可叠加',
  must_skip: '仅快速拉到底触发全拉',
  may_skip_r: '轻拉立即响应，快速全拉时替换',
  must_skip_r: '轻拉立即响应，仅快拉可切换',
};
const stageDefaults: TriggerStagesConfig = {
  left_mode:'no_skip', right_mode:'no_skip', soft_threshold:0.25, skip_delay_ms:150,
};
const stageChoices = [{options:Object.entries(triggerModes).map(([value, label]) => ({value,label}))}];

export function TriggerSettings({profile, layerMode, update, onEditBase}: {
  profile: Profile;
  layerMode: boolean;
  update: MotionUpdate;
  onEditBase: () => void;
}) {
  const stage = profile.trigger_stages ?? stageDefaults;
  const updateStages = (patch: Partial<TriggerStagesConfig>) => update(draft => {
    draft.trigger_stages = {...(draft.trigger_stages ?? stageDefaults), ...patch};
  });
  const skipping = [stage.left_mode, stage.right_mode].some(mode => mode.startsWith('may_skip') || mode.startsWith('must_skip'));
  return <div className={settingStyles.page}>
    <SettingSection id="trigger-stages" title="扳机轻拉与全拉" defaultOpen summary={layerMode?'基础层共用':'轻拉与全拉'}>
      <p className={styles.hint}>在动作里分别选择 LT 或 RT 的轻拉和全拉。全拉点取决于校准后的最大行程。</p>
      {layerMode && <div className="studio-actions">
        <p className={styles.hint}>轻拉和全拉规则在基础层设置，所有配置层共用。</p>
        <button type="button" className="secondary-btn" onClick={onEditBase}>编辑基础层</button>
      </div>}
      <fieldset className="studio-content" disabled={layerMode}>
        <div className={styles.grid}>
          <SettingField label="左扳机全拉模式"><OptionPicker label="左扳机全拉模式" value={stage.left_mode} groups={stageChoices}
            onChange={value => updateStages({left_mode:value as TriggerStageMode})}/></SettingField>
          <SettingField label="右扳机全拉模式"><OptionPicker label="右扳机全拉模式" value={stage.right_mode} groups={stageChoices}
            onChange={value => updateStages({right_mode:value as TriggerStageMode})}/></SettingField>
          {stage.soft_threshold >= 0 && <Range label="轻拉阈值" value={stage.soft_threshold} min={0} max={0.99} step={0.01}
            onChange={soft_threshold => updateStages({soft_threshold})}/>}
          {skipping && <Range label="快速全拉时间窗" value={stage.skip_delay_ms} min={0} max={5000} step={1} unit=" ms" slider={false}
            onChange={skip_delay_ms => updateStages({skip_delay_ms})}/>}
        </div>
        <Toggle label="随扳机移动触发轻拉" value={stage.soft_threshold < 0}
          onChange={enabled => updateStages({soft_threshold:enabled ? -1 : stageDefaults.soft_threshold})}/>
      </fieldset>
    </SettingSection>
    <TriggerInputSettings profile={profile} update={update}/>
  </div>;
}
