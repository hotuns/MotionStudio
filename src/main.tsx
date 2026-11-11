import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import {FloatingBindings} from './studio/FloatingBindings';
import './style.css';
const content = '__MOTION_BINDING_WINDOW__' in window ? <FloatingBindings /> : <App />;
ReactDOM.createRoot(document.getElementById('root')!).render(<React.StrictMode>{content}</React.StrictMode>);
