import {useEffect, useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {inputLabel, outputLabel, type Profile} from '../types';
import {
  loadCommunityProfile, maxShareCodeLength, resolveCommunityCode,
  type CommunityProfile, type SharedPreview, type SharedSource,
} from '../profile-sharing';
import {Dialog, gamepadNames} from './Controls';
import styles from './ProfileManager.module.css';

type Lookup = {code: string} | {entry: CommunityProfile};
type Result = {source: SharedSource; preview: SharedPreview; entry?: CommunityProfile};

async function inspect(lookup: Lookup, signal: AbortSignal): Promise<Result> {
  const entry = 'entry' in lookup ? lookup.entry :
    /^MS-[a-f0-9]{8}$/i.test(lookup.code) ? await resolveCommunityCode(lookup.code, signal) : undefined;
  const source: SharedSource = entry ? await loadCommunityProfile(entry, signal) :
    {kind: 'code', code: 'code' in lookup ? lookup.code : ''};
  signal.throwIfAborted();
  const preview = await invoke<SharedPreview>('preview_shared_profile', {source});
  if (entry && (entry.name !== preview.profile.name || entry.output !== preview.profile.virtual_gamepad)) {
    throw Error('社区配置与目录信息不一致');
  }
  return {source, preview, entry};
}

function ProfilePreview({profile}: {profile: Profile}) {
  const {gyro} = profile;
  const mode = gyro.motion.mode === 'steering' ? '姿态控制摇杆' :
    gyro.motion.mode === 'stick_rotation' ? '体感旋转摇杆方向' :
    gyro.output === 'mouse' ? '鼠标瞄准' : '右摇杆瞄准';
  const groups = [{id: '', name: '基础层', actions: profile.actions}, ...(profile.layers ?? [])];
  return <>
    <dl className={styles.shareSummary}>
      <div><dt>输出方式</dt><dd>{gamepadNames[profile.virtual_gamepad]}</dd></div>
      <div><dt>体感控制</dt><dd>{gyro.enabled ? mode : '体感已关闭'}</dd></div>
      {gyro.enabled && <>
        <div><dt>开启方式</dt><dd>{gyro.activation === 'always' ? '始终开启' : <>
          {gyro.activation_mode === 'hold' ? '按住开启' : '按一下切换开关'} · {inputLabel(gyro.activation)}
        </>}</dd></div>
        <div><dt>体感灵敏度</dt><dd>{Number(gyro.sensitivity.toFixed(2))}×</dd></div>
      </>}
    </dl>
    {groups.some(group => group.actions.length > 0) && <details className={styles.shareBindings}>
      <summary>查看动作映射</summary>
      {groups.filter(group => group.actions.length > 0).map(group => <section key={group.id}>
        {groups.length > 1 && <h4 translate={group.id ? 'no' : undefined}>{group.name}</h4>}
        <ul>{group.actions.map((action, index) => <li key={index}>
          <strong translate="no">{action.name}</strong>
          <span>{inputLabel(action.input)} → {outputLabel(action.output)}</span>
        </li>)}</ul>
      </section>)}
    </details>}
  </>;
}

export function ProfileShareDialog({community, onClose, onImport}: {
  community?: CommunityProfile;
  onClose: () => void;
  onImport: (source: SharedSource) => Promise<void>;
}) {
  const [code, setCode] = useState('');
  const [lookup, setLookup] = useState<Lookup | null>(community ? {entry: community} : null);
  const [result, setResult] = useState<Result | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(!!community);
  const [operation, setOperation] = useState<'paste' | 'copy' | 'save' | null>(null);
  const [copied, setCopied] = useState(false);
  const processing = loading || !!operation;

  useEffect(() => {
    if (!lookup) return;
    const request = new AbortController();
    setLoading(true);
    setError('');
    setCopied(false);
    setResult(null);
    void inspect(lookup, request.signal).then(value => {
      if (!request.signal.aborted) setResult(value);
    }).catch(error => {
      if (!request.signal.aborted) setError(error instanceof Error ? error.message : String(error));
    }).finally(() => {
      if (!request.signal.aborted) setLoading(false);
    });
    return () => request.abort();
  }, [lookup]);

  function startLookup(value: string) {
    const trimmed = value.trim();
    if (!trimmed.startsWith('MS1.') && !/^MS-[a-f0-9]{8}$/i.test(trimmed)) {
      setError('请输入有效的配置码');
      return;
    }
    setLoading(true);
    setLookup({code: trimmed});
  }

  async function paste() {
    if (processing) return;
    setOperation('paste');
    setError('');
    try {
      const value = await invoke<string>('read_profile_code');
      setCode(value);
      startLookup(value);
    } catch (error) { setError(String(error)); }
    finally { setOperation(null); }
  }

  async function submit() {
    if (processing) return;
    if (!result) {
      if (community) {
        setLoading(true);
        setLookup({entry: community});
      } else startLookup(code);
      return;
    }
    setOperation('save');
    setError('');
    try {
      await onImport(result.source);
      onClose();
    } catch (error) {
      setError(String(error));
      setOperation(null);
    }
  }

  async function copyShortCode(entry: CommunityProfile) {
    if (processing) return;
    setOperation('copy');
    setError('');
    try {
      await invoke('copy_community_code', {code: entry.code});
      setCopied(true);
    } catch (error) { setError(String(error)); }
    finally { setOperation(null); }
  }

  const close = () => { if (operation !== 'save') onClose(); };
  return <Dialog title={community ? '预览配置' : '输入配置码'} onClose={close} onSubmit={() => void submit()} error={error}
    description={result ? '添加后可在本地配置中编辑，由你决定何时应用。' :
      community ? undefined : '支持社区短码和好友分享的完整配置码。'}
    actions={<>
      <button type="button" className="secondary-btn" disabled={operation === 'save'} onClick={close}>取消</button>
      {result && !community && <button type="button" className="secondary-btn" disabled={processing}
        onClick={() => { setLookup(null); setResult(null); setError(''); }}>换一个配置码</button>}
      <button type="submit" className="primary-btn" disabled={processing || (!community && !result && !code.trim())}>
        {result ? '添加到本地' : community ? '重试' : '预览配置'}
      </button>
    </>}>
    {!community && !result && <div className={styles.codeField}>
      <label htmlFor="studio-profile-code">配置码</label>
      <textarea id="studio-profile-code" value={code} maxLength={maxShareCodeLength} spellCheck={false}
        placeholder="MS-1234ABCD" disabled={processing} onChange={event => { setCode(event.target.value); setError(''); }}/>
      <button type="button" className="secondary-btn" disabled={processing} onClick={() => void paste()}>粘贴并预览</button>
    </div>}
    {loading && <p role="status">正在检查配置…</p>}
    {result && <>
      <h3 className={styles.shareName} translate="no">{result.preview.profile.name}</h3>
      {result.entry && <div className={styles.shareSource}>
        <p translate="no">{result.entry.game} · {result.entry.controller} · {result.entry.author}</p>
        <p translate="no">{result.entry.description}</p>
        {result.entry.notes && <p className="studio-note" translate="no">{result.entry.notes}</p>}
        <button type="button" className="secondary-btn" disabled={processing} onClick={() => void copyShortCode(result.entry!)}>
          复制短码 · {result.entry.code}
        </button>
        {copied && <p role="status">配置码已复制</p>}
      </div>}
      <ProfilePreview profile={result.preview.profile}/>
      {result.preview.custom_filters > 0 && <p className="studio-note">
        此配置包含自定义滤波脚本。导入后脚本为关闭状态，可在进阶设置中查看。
      </p>}
    </>}
  </Dialog>;
}
