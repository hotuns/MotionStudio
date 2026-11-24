import type {ResponsePoint, StickResponsePoint} from './types';

export type BezierDomain = 'game' | 'acceleration' | 'action' | 'stick_mouse';
export const bounds = (domain: BezierDomain): ResponsePoint => domain==='acceleration' ? [4000,10] : [1,1];
export const fixedAnchor = (domain: BezierDomain, i: number, points: ResponsePoint[]) =>
  (domain==='game' || domain==='stick_mouse') && (i===0 || i===points.length-1);
const clamp = (n: number, min: number, max: number) => Math.min(max,Math.max(min,n));

/** Seed straight spans exactly. For a legacy power span fit two handles through interior samples. */
export function bezierFromKnots(knots: readonly StickResponsePoint[]): ResponsePoint[] {
  const result: ResponsePoint[] = [[knots[0][0],knots[0][1]]];
  for (let i=1; i<knots.length; ++i) {
    const [x,y,power=1]=knots[i-1], [endX,endY]=knots[i];
    const dx=endX-x, dy=endY-y;
    const a=(1/3)**power, b=(2/3)**power;
    const c1=power===1?1/3:clamp(3*a-1.5*b+1/3,0,1);
    const c2=power===1?2/3:clamp(3*b-1.5*a-5/6,0,1);
    result.push([x+dx/3,y+dy*c1],[x+2*dx/3,y+dy*c2],[endX,endY]);
  }
  return result;
}

/** Seed an editable acceleration curve from the native preset preview. Keep
 * its turns without giving the user hundreds of nearly collinear anchors. */
export function bezierFromGainPreview(samples: [number,number,number][]) {
  const points: ResponsePoint[]=[];
  for (const [x,,gain] of samples) {
    if (x<=4000 && (!points.length || x>points[points.length-1][0])) points.push([x,gain]);
  }
  if (points.length<2) return undefined;
  const kept=[0,points.length-1];
  while (kept.length<16) {
    let error=0.005, insertAt=0, next=-1;
    for (let j=1;j<kept.length;j++) {
      const left=kept[j-1],right=kept[j], [x,y]=points[left], end=points[right];
      for (let i=left+1;i<right;i++) {
        const fraction=(points[i][0]-x)/(end[0]-x);
        const deviation=Math.abs(points[i][1]-(y+fraction*(end[1]-y)));
        if (deviation>error) { error=deviation; insertAt=j; next=i; }
      }
    }
    if (next<0) break;
    kept.splice(insertAt,0,next);
  }
  return bezierFromKnots(kept.map(i=>points[i]));
}

// One representable f32 beyond a nonnegative neighbour. Imported anchors can
// be much closer than the UI's normal drag or keyboard increment.
function nextFloat(value: number, direction: 1 | -1) {
  const f = new Float32Array([value]);
  if (f[0]===0 && direction<0) return -Infinity;
  new Uint32Array(f.buffer)[0]+=direction;
  return f[0];
}

function monotone(a: number,b: number,c: number,d: number) {
  return b>=a && c<=d && c-b>=-Math.sqrt((b-a)*(d-c));
}

/** Keep valid, possibly crossed tangents produced by an exact subdivision. */
function constrainSegment(curve: ResponsePoint[], start: number, axis: 0|1) {
  const [a,b,c,d]=curve.slice(start,start+4).map(p=>p[axis]);
  if (!monotone(a,b,c,d)) {
    curve[start+1][axis]=clamp(b,a,d);
    curve[start+2][axis]=clamp(c,a,d);
  }
}

export function moveBezier(points: ResponsePoint[], i: number, x: number, y: number, domain: BezierDomain) {
  if (fixedAnchor(domain,i,points) || (x===points[i][0] && y===points[i][1])) return points;
  const [maxX,maxY]=bounds(domain);
  const next=points.map((p):ResponsePoint=>[...p]);
  if (i%3!==0) {
    const start=i-i%3, a=points[start], b=points[start+3];
    next[i]=[x===points[i][0]?x:clamp(x,a[0],b[0]), y===points[i][1]?y:clamp(y,domain==='game'?a[1]:0,domain==='game'?b[1]:maxY)];
    constrainSegment(next,start,0);
    if (domain==='game') constrainSegment(next,start,1);
    return next;
  }
  const prev=points[i-3], after=points[i+3];
  next[i]=[clamp(x,prev?nextFloat(prev[0],1):0,after?nextFloat(after[0],-1):maxX),
    clamp(y,domain==='game' && prev?prev[1]:0,domain==='game' && after?after[1]:maxY)];
  for (const start of [i-3,i]) {
    if (start<0 || start+3>=points.length) continue;
    for (const axis of [0,1] as const) {
      if (next[i][axis]===points[i][axis]) continue;
      const a=points[start][axis], b=points[start+3][axis];
      const na=next[start][axis], nb=next[start+3][axis];
      for(let k=1;k<=2;k++) {
        if (axis===1 && domain!=='game') {
          if ((k===1?start:start+3)===i) next[start+k][axis]=clamp(points[start+k][axis]+next[i][axis]-points[i][axis],0,maxY);
        } else {
          const fraction=b===a?k/3:(points[start+k][axis]-a)/(b-a);
          next[start+k][axis]=clamp(na+fraction*(nb-na),0,axis===0?maxX:maxY);
        }
      }
      if (axis===0 || domain==='game') constrainSegment(next,start,axis);
    }
  }
  return next;
}

export function removeBezier(points: ResponsePoint[], index: number, domain: BezierDomain) {
  const next=points.map((p):ResponsePoint=>[...p]);
  next.splice(index-1,3);
  constrainSegment(next,index-3,0);
  if (domain==='game') constrainSegment(next,index-3,1);
  return {points:next,index:index-3};
}
