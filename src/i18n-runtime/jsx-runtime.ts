import {jsx as createJsx,jsxs as createJsxs} from 'react/jsx-runtime';
import {localizeProps} from './props';
export {Fragment,type JSX} from 'react/jsx-runtime';
export const jsx:typeof createJsx=(type,props,key)=>createJsx(type,localizeProps(type,props),key);
export const jsxs:typeof createJsxs=(type,props,key)=>createJsxs(type,localizeProps(type,props),key);
