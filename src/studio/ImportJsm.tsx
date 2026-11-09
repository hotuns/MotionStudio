import {useState} from 'react';
import {createPortal} from 'react-dom';
import {invoke} from '@tauri-apps/api/core';
import type {Entry, JsmFile, JsmReport} from '../types';
import {Dialog} from './Controls';
import {decodeConfig, ImportFiles} from './ImportFiles';
import styles from './ImportJsm.module.css';

const statusNames = {converted:'已转换', approximate:'需要核验', skipped:'未迁移', ignored:'程序设置', overridden:'已覆盖', included:'已展开'};

export function ImportJsm({onClose, onImported}: {onClose: () => void; onImported: (entry: Entry) => Promise<void>}) {
  const [files, setFiles] = useState<JsmFile[]>([{name:'JSM 配置.txt', text:''}]);
  const [selected, setSelected] = useState(0);
  const [picker, setPicker] = useState<'main' | 'related' | null>(null);
  const [name, setName] = useState('');
  const [encoding, setEncoding] = useState('');
  const [report, setReport] = useState<JsmReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [filter, setFilter] = useState('review');
  const legacy = /\.toml$/i.test(files[0].name);
  const ready = !!report && report.converted + report.approximate > 0;
  const file = files[selected];
  const clearReport = () => { setReport(null); setError(''); };

  function choose(kind: 'main' | 'related', text: string, name: string, encoding: string) {
    clearReport();
    setPicker(null);
    const next = {name, text};
    if (kind === 'main') {
      setFiles([next, ...(/\.toml$/i.test(name) ? [] : files.slice(1))]);
      setSelected(0);
      setEncoding(encoding);
    } else {
      setFiles([...files, next]);
      setSelected(files.length);
    }
  }

  async function preview() {
    setBusy(true);
    clearReport();
    try {
      const result = await invoke<JsmReport>('preview_jsm', {files});
      setReport(result);
      setName(result.profile.name);
      setFilter(result.approximate + result.skipped ? 'review' : 'all');
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function create() {
    if (!ready || !name.trim()) return;
    setBusy(true);
    setError('');
    try {
      const entry = await invoke<Entry>('import_jsm', {files, name:name.trim()});
      await onImported(entry);
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
    }
  }

  const lines = report?.lines.filter(line => filter === 'all'
    || (filter === 'review' && ['approximate', 'skipped'].includes(line.status)) || filter === line.status) ?? [];
  return <Dialog title="导入 JSM 配置" onClose={onClose} error={error}
    onSubmit={() => { if (!busy && !picker) void (report ? create() : preview()); }}
    description="将支持的设置转换成可在 Studio 动作列表里编辑的配置。"
    actions={<>
      <button type="button" className="secondary-btn" onClick={onClose}>取消导入</button>
      {report
        ? <button type="submit" className="primary-btn" disabled={busy || !ready || !name.trim()}>创建新配置</button>
        : <button type="submit" className="primary-btn" disabled={busy || !files[0].text.trim()}>预览转换</button>}
    </>}>
    {!report ? <>
      <div className={styles.tools}>
        <button type="button" className="secondary-btn" disabled={busy} onClick={() => setPicker('main')}>选择 JSM 文件</button>
        <label className="secondary-btn">从系统选择<input type="file" hidden disabled={busy} accept=".txt,.jsm,.cfg,.conf,.toml"
          onChange={event => {
            const file = event.target.files?.[0];
            event.target.value = '';
            if (!file) return;
            void file.arrayBuffer().then(buffer => {
              const value = decodeConfig(new Uint8Array(buffer));
              choose('main', value.text, file.name, value.encoding);
            }).catch(error => setError(String(error)));
          }}/></label>
        {!legacy && <button type="button" className="secondary-btn" disabled={busy || files.length >= 16}
          onClick={() => setPicker('related')}>添加关联 JSM 文件</button>}
      </div>
      {legacy ? <p><strong translate="no">{files[0].name}</strong><br/>旧版配置及内含的关联文件会一起转换。</p> : <>
        {files.length > 1 && <div className={styles.tabs} role="group" aria-label="JSM 文件">
          {files.map((file, i) => <button key={i} type="button" className="secondary-btn" aria-pressed={i === selected}
            disabled={busy} onClick={() => setSelected(i)}>
            {i === 0 && <span>主文件 · </span>}<span translate="no">{file.name}</span>
          </button>)}
          {selected > 0 && <button type="button" className="ghost-btn" disabled={busy} onClick={() => {
            setFiles(files.filter((_, i) => i !== selected));
            setSelected(selected - 1);
          }}>移除关联文件</button>}
        </div>}
        <label className={styles.field}>JSM 配置内容
          <textarea aria-label="JSM 配置内容" rows={12} spellCheck={false} value={file.text} disabled={busy}
            placeholder={'TRIGGER_THRESHOLD = 0.25\nZR_MODE = NO_SKIP\nZR = RMOUSE\nZRF = LMOUSE'}
            onChange={event => setFiles(files.map((file, i) => i === selected ? {...file, text:event.target.value} : file))}/>
        </label>
        <p>可以粘贴配置，或选择文件。主文件引用的配置可作为关联文件添加。</p>
      </>}
      {picker && createPortal(<ImportFiles kind="jsm" allowLegacy={picker === 'main'} onClose={() => setPicker(null)}
        onSelect={(text, name, encoding) => choose(picker, text, name, encoding)}/>, document.body)}
    </> : <>
      <div className={styles.source}><strong translate="no">{files[0].name}</strong><span>{encoding}</span>
        <button type="button" className="ghost-btn" disabled={busy} onClick={clearReport}>修改原文</button></div>
      <label className={styles.field}>新配置名称<input autoFocus aria-label="导入后的配置名称" maxLength={100}
        value={name} disabled={busy} onChange={event => setName(event.target.value)}/></label>
      <div className={styles.stats}><span><strong>{report.converted}</strong> 已转换</span>
        <span><strong>{report.approximate}</strong> 需要核验</span><span><strong>{report.skipped}</strong> 未迁移</span></div>
      <p>{report.profile.actions.length} 个动作 · 体感{report.profile.gyro.enabled?'已配置':'关闭'} · {report.profile.virtual_gamepad==='none'?'键鼠输出':report.profile.virtual_gamepad==='xbox_360'?'虚拟 Xbox 360':'虚拟 DualShock 4'}</p>
      {report.warnings.map((warning, i) => <p className="studio-note" key={i}>{warning}</p>)}
      <label className={styles.field}>逐行报告<select aria-label="筛选转换报告" value={filter} onChange={event => setFilter(event.target.value)}>
        <option value="review">需要处理或核验</option><option value="all">全部命令</option>
        <option value="converted">已转换</option><option value="skipped">未迁移</option>
        <option value="included">已展开</option><option value="ignored">程序设置</option><option value="overridden">已被覆盖</option>
      </select></label>
      <div className={styles.report} aria-label="JSM 转换报告">{lines.map((line, index) => <article key={index} data-status={line.status}>
        <div><strong><span translate="no">{line.file}</span>{' · '}{'第 '+line.line+' 行'}</strong><span>{statusNames[line.status]}</span></div>
        <code translate="no">{line.source}</code><p>{line.message}</p>
      </article>)}{!lines.length && <p>没有此类项目。</p>}</div>
    </>}
  </Dialog>;
}
