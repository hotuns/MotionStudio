import {t} from '../i18n';
import {componentNames,componentStateNames,type ComponentPlan,type InstallableComponent} from '../component-setup';
import {Dialog} from './Controls';
import {StudioIcon} from './StudioIcon';
import styles from './ComponentSetup.module.css';

type StatusProps={plan:ComponentPlan;checking:boolean;error:string|null;rebootRequired:boolean};
export function ComponentSetupNotice({plan,checking,error,rebootRequired,blocked,onOpen}:StatusProps&{blocked:boolean;onOpen:()=>void}) {
  if(plan.ready&&!error&&!rebootRequired)return null;
  const pending=!plan.checked&&!error;
  return <aside className={styles.notice} aria-label="组件安装提醒">
    <StudioIcon name="settings" size={21}/>
    <div role="status"><strong>{rebootRequired?'组件变更需要重启 Windows':pending?'正在检查所需组件…':'开始映射前，请先准备组件'}</strong>
      <p>{rebootRequired?'重启电脑后再打开 Motion Studio，完成组件检测。':error?'组件检测未完成，请重新检测或查看组件设置。':pending?'根据当前配置检查输出和隐藏手柄所需的组件。':t('待处理：{0}').replace('{0}',plan.issues.map(item=>componentNames[item.kind]).join('、'))}</p>
    </div>
    <button type="button" className="primary-btn" disabled={blocked||checking} onClick={onOpen}>组件安装指引<StudioIcon name="arrow" size={14}/></button>
  </aside>;
}

type Props=StatusProps&{
  profileName?:string;busy:boolean;outputActive:boolean;installing:InstallableComponent|null;
  result:{message:string;error:boolean}|null;
  onInstall:(kind:InstallableComponent)=>void;onInstallAll:()=>void;onRefresh:()=>void;onClose:()=>void;
  onRepairHidHide:()=>void;repairingHidHide:boolean;
  onSettings:()=>void;onUsbip:()=>void;onStop:()=>void;
};
export function ComponentSetup({plan,checking,error,rebootRequired,profileName,busy,outputActive,installing,result,onInstall,onInstallAll,onRefresh,onClose,onRepairHidHide,repairingHidHide,onSettings,onUsbip,onStop}:Props) {
  const locked=busy||checking;
  return <Dialog title="组件安装指引" className={styles.dialog} onClose={onClose} actions={<>
    <button type="button" className="secondary-btn" disabled={locked} onClick={onRefresh}>重新检测组件</button>
    <button type="button" className={plan.installable.length?'secondary-btn':'primary-btn'} disabled={busy} onClick={onClose}>{plan.ready&&!rebootRequired?'完成':'稍后设置'}</button>
    {!!plan.installable.length&&<button type="button" className="primary-btn" disabled={locked||outputActive} onClick={onInstallAll}>{installing?t('正在安装：{0}').replace('{0}',componentNames[installing]):'一键安装所需组件'}</button>}
  </>}>
    <div className={styles.intro}>
      <span className={styles.icon}><StudioIcon name="settings" size={26}/></span>
      <div><h3>{plan.ready&&!rebootRequired?'所需组件已就绪':'先准备组件，再开始映射'}</h3>
        <p>安装 Motion Studio 后，还需要为所用功能准备系统组件。下方只列出当前配置需要的项目。</p></div>
    </div>
    <div className={styles.scope}><span>当前配置</span><strong translate="no">{profileName??t('尚未选择配置')}</strong>
      <span>{t('已就绪 {0} / {1}').replace('{0}',String(plan.items.length-plan.issues.length)).replace('{1}',String(plan.items.length))}</span></div>
    {checking&&<p className={styles.feedback} role="status">正在检查所需组件…</p>}
    {error&&<div className={styles.feedback} role="alert"><strong>组件检测未完成，请重新检测或查看组件设置。</strong><p>{error}</p></div>}
    {result&&<p className={styles.feedback} role={result.error?'alert':'status'}>{result.message}</p>}
    {rebootRequired&&<p className={styles.feedback} role="status">请保存工作并重启 Windows，以完成驱动变更。</p>}
    {outputActive&&<div className={styles.feedback}><p>停止映射或观察后可安装或修复组件。</p>
      <button type="button" className="secondary-btn" disabled={locked} onClick={onStop}>停止映射或观察</button></div>}
    <ol className={styles.items}>{plan.items.map((item,index)=><li key={item.kind} data-component={item.kind}>
      <span className={styles.number} data-ready={item.status==='running'}>{item.status==='running'?<StudioIcon name="check" size={17}/>:index+1}</span>
      <div className={styles.itemCopy}><div className={styles.title}><strong translate="no">{componentNames[item.kind]}</strong>
        <span data-ready={item.status==='running'}>{componentStateNames[item.status]??item.status}</span></div><p>{item.purpose}</p>
        {item.status==='stopped'&&<p>已安装但尚未运行。先重启 Windows，再重新检测；仍未就绪时到组件设置中检查。</p>}
        {item.kind!=='hidhide'&&['partial','incompatible'].includes(item.status)&&<p>可在组件管理中查看原因并卸载，或使用原安装程序修复。</p>}
        {item.kind==='usbip'&&item.status!=='running'&&<p>此组件需要从官方项目安装。按官方说明完成后返回这里重新检测。</p>}
        {item.error&&<p className={styles.error}>{item.error}</p>}
      </div>
      {item.kind==='usbip'?item.status!=='running'&&<button type="button" className="secondary-btn" disabled={busy} onClick={onUsbip}>打开官方安装说明</button>
        :item.status==='missing'&&<button type="button" className="secondary-btn" disabled={locked||outputActive}
          onClick={()=>onInstall(item.kind as InstallableComponent)}>{installing===item.kind?'正在安装…':t('安装 {0}').replace('{0}',componentNames[item.kind])}</button>}
      {item.kind==='hidhide'&&!['running','missing','restart_required','unsupported'].includes(item.status)&&
        <button type="button" className="secondary-btn" disabled={locked||outputActive} onClick={onRepairHidHide}>
          {repairingHidHide?'正在修复 HidHide…':'一键修复 HidHide'}
        </button>}
    </li>)}</ol>
    {!plan.items.length&&<p>当前输出方式无需额外安装组件，可以继续检查手柄输入和配置。</p>}
    {plan.ready&&!rebootRequired?<p className={styles.tip}>组件已就绪。连接手柄、选择配置后，可以启动映射。</p>
      :!!plan.installable.length&&<p className={styles.tip}>点击一键安装后，按 Windows 提示授权，软件会依次安装并检测所需组件。需要重启时会提示；取消授权后可以稍后继续。</p>}
    <button type="button" className={styles.settings} disabled={busy} onClick={onSettings}>查看全部组件与输出设置<StudioIcon name="arrow" size={14}/></button>
  </Dialog>;
}
