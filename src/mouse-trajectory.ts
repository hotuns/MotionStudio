import type {MouseTracePoint} from './types';

/** Fit relative output counts to a plot with equal horizontal and vertical scale. */
export function mouseTrajectory(points:MouseTracePoint[],width:number,height:number,padding:number) {
  const first=points[0]?.position??[0,0];
  const last=points.at(-1)?.position??first;
  let [minX,minY]=first;
  let [maxX,maxY]=first;
  for(const {position:[x,y]} of points){
    minX=Math.min(minX,x);maxX=Math.max(maxX,x);
    minY=Math.min(minY,y);maxY=Math.max(maxY,y);
  }
  const scale=Math.min((width-padding*2)/Math.max(80,maxX-minX),(height-padding*2)/Math.max(36,maxY-minY));
  const project=([x,y]:[number,number]):[number,number]=>[
    width/2+(x-(minX+maxX)/2)*scale,height/2+(y-(minY+maxY)/2)*scale,
  ];
  return {
    path:points.map(({position})=>project(position).map(value=>value.toFixed(1)).join(',')).join(' '),
    start:project(first),end:project(last),delta:[last[0]-first[0],last[1]-first[1]],
  };
}

export const signedCount=(value:number)=>value>0?'+'+value:String(value);
