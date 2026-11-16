import {t} from '../i18n';
import styles from './StickOverlapPreview.module.css';

export function StickOverlapPreview({degrees,label,onReset}:{degrees:number;label:string;onReset:()=>void}) {
  const point=(angle:number,radius=58)=>{
    const radians=angle*Math.PI/180;
    return [Math.sin(radians)*radius,-Math.cos(radians)*radius];
  };
  return <div className={styles.preview}>
    <svg viewBox="-64 -64 128 128" role="img" aria-label={t(label)}>
      {['↑','↗','→','↘','↓','↙','←','↖'].map((arrow,index)=>{
        const diagonal=index%2===1,width=diagonal?degrees:90-degrees;
        if(width<=0)return null;
        const start=point(index*45-width/2),end=point(index*45+width/2),text=point(index*45,42);
        return <g key={arrow}>
          <path className={diagonal?styles.diagonal:styles.cardinal}
            d={`M0,0 L${start.join(',')} A58,58 0 0,1 ${end.join(',')} Z`}/>
          {width>=8&&<text x={text[0]} y={text[1]} textAnchor="middle" dominantBaseline="central">{arrow}</text>}
        </g>;
      })}
    </svg>
    <div className={styles.legend}>
      <span><i className={styles.cardinal} aria-hidden="true"/>单方向</span>
      <span><i className={styles.diagonal} aria-hidden="true"/>相邻双方向</span>
      <button type="button" className="secondary-btn" onClick={onReset}>恢复默认角度</button>
    </div>
  </div>;
}
