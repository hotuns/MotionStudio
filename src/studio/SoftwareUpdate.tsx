import { useState } from 'react';
import { version } from '../../package.json';
import { t } from '../i18n';
import { Dialog } from './Controls';
import type { StudioUpdate } from './software-update';
import styles from './SettingsPage.module.css';

export function SoftwareUpdate({ update, blocked, onInstall, onOpenReleases }: {
  update: StudioUpdate;
  blocked: string | null;
  onInstall: () => void;
  onOpenReleases: () => void;
}) {
  const [confirm, setConfirm] = useState(false);
  const { result, release, activity, transfer, error } = update;
  const portable = result?.mode === 'portable';
  const development = result?.mode === 'development';
  const downloading = activity === 'downloading';
  const installing = activity === 'installing';
  const ready = activity === 'ready';
  const downloadPercent = transfer?.total ? Math.min(100, Math.floor(100 * transfer.received / transfer.total)) : undefined;
  const installLabel = portable ? '解压并重启' : '安装并重启';

  return <section className={styles.section} aria-labelledby="software-update-heading">
    <div className={styles.heading}>
      <div>
        <h2 id="software-update-heading">软件更新</h2>
        <p>{t('当前版本 v{0}').replace('{0}', version)}
          {result && <> · {development ? '开发版' : portable ? '免安装版' : '安装版'}</>}
        </p>
      </div>
      <button type="button" className="secondary-btn"
        disabled={development || !!activity} onClick={() => void update.check()}>检查更新</button>
    </div>
    <div role="status" aria-live="polite">
      {activity === 'checking' && <p>正在检查更新…</p>}
      {result?.state === 'current' && <p>已是最新版本。</p>}
      {result?.state === 'unpublished' && <p>暂未发布可用的在线更新。</p>}
      {development && <p>开发模式下不执行在线更新。</p>}
      {release && <p className={styles.updateStatus}>
        <strong>{t('发现新版本 v{0}').replace('{0}', release.version)}</strong>
        {ready && <> · <span>已下载并验证</span></>}
        {installing && <> · <span>正在准备重启…</span></>}
      </p>}
    </div>
    {downloading && <>
      <label htmlFor="software-update-progress">
        {downloadPercent === 100 ? '正在验证更新包…' : '正在下载更新…'}
        {downloadPercent !== undefined && <span> {downloadPercent}%</span>}
        {downloadPercent === undefined && transfer && <span> {(transfer.received / (1024 * 1024)).toFixed(1)} MB</span>}
      </label>
      <progress id="software-update-progress" className={styles.progress} value={downloadPercent} max={100}/>
    </>}
    {release?.notes && <details className={styles.updateNotes}>
      <summary>版本说明</summary>
      <pre translate="no">{release.notes}</pre>
    </details>}
    {error && <p className={styles.error} role="alert">{error}</p>}
    {ready && blocked && <p className={styles.notice}>{blocked}</p>}
    <div className={styles.actions}>
      {release && !activity && <button type="button" className="primary-btn" onClick={() => void update.download()}>下载更新</button>}
      {ready && <button type="button" className="primary-btn" disabled={!!blocked} onClick={() => setConfirm(true)}>{installLabel}</button>}
      <button type="button" className="secondary-btn" disabled={installing} onClick={onOpenReleases}>打开 GitHub 发布页</button>
    </div>
    {confirm && release && <Dialog title={installLabel} onClose={() => setConfirm(false)}
      description={t('将更新到 v{0}。运行中的映射会停止，已保存的配置会保留。').replace('{0}', release.version)}
      onSubmit={() => { if (!blocked) { setConfirm(false); onInstall(); } }}
      actions={<>
        <button type="button" className="secondary-btn" autoFocus onClick={() => setConfirm(false)}>取消</button>
        <button type="submit" className="primary-btn" disabled={!!blocked}>{installLabel}</button>
      </>}>
      {portable && <p>新版会解压到旁边的新文件夹并自动打开，旧版文件夹会保留。</p>}
    </Dialog>}
  </section>;
}
