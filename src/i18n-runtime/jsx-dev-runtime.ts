import {jsxDEV as create} from 'react/jsx-dev-runtime';
import {localizeProps} from './props';
export {Fragment,type JSX} from 'react/jsx-dev-runtime';
export const jsxDEV:typeof create=(type,props,...args)=>create(type,localizeProps(type,props),...args);
