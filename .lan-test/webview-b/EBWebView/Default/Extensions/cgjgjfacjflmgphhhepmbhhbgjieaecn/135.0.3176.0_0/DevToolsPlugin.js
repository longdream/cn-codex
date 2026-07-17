// Copyright 2025 The Chromium Authors. All rights reserved.
// Copyright (C) Microsoft Corp. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.
import { NamedFunctionRange } from './NamedFunctionRange.js';
import * as ts from './third_party/typescript/typescript.js';
const tsc = ts.default;
function parse(fileName, source) {
    tsc.createSourceFile(fileName, source, tsc.ScriptTarget.ESNext, /* setParentNodes: */ true);
    const markName = `parsing: ${fileName}`;
    const endMarkName = `${markName}-end`;
    performance.mark(markName);
    const kind = getFileType(fileName);
    const tsSource = tsc.createSourceFile(fileName, source, tsc.ScriptTarget.ESNext, /* setParentNodes: */ true, kind);
    const result = visitRoot(tsSource, fileName);
    performance.mark(endMarkName);
    performance.measure(fileName, markName, endMarkName);
    return result;
}
function getFileType(fileName) {
    const lowered = fileName.toLowerCase();
    if (lowered.endsWith('.tsx')) {
        return tsc.ScriptKind.TSX;
    }
    if (lowered.endsWith('.ts')) {
        return tsc.ScriptKind.TS;
    }
    if (lowered.endsWith('.jsx')) {
        return tsc.ScriptKind.JSX;
    }
    if (lowered.endsWith('.js') || lowered.endsWith('.cjs') || lowered.endsWith('.mjs')) {
        return tsc.ScriptKind.JS;
    }
    return tsc.ScriptKind.TS; // default to ts
}
function visitRoot(source, fileName) {
    const accumulator = [];
    const name = `globalCode: ${fileName}`;
    accumulator.push(createDescriptor(name, source, source));
    for (const child of source.getChildren()) {
        visitNodeIterative(accumulator, child, source);
    }
    return accumulator;
}
function visitNodeIterative(dest, node, source) {
    if (tsc.isFunctionDeclaration(node) || tsc.isFunctionExpression(node) || tsc.isMethodDeclaration(node) ||
        tsc.isArrowFunction(node) || tsc.isConstructorDeclaration(node) || tsc.isGetAccessor(node) ||
        tsc.isGetAccessorDeclaration(node) || tsc.isSetAccessor(node) || tsc.isSetAccessorDeclaration(node)) {
        visitFunctionNodeImpl(dest, node, source);
    }
    for (const child of node.getChildren()) {
        visitNodeIterative(dest, child, source);
    }
}
function visitFunctionNodeImpl(dest, node, source) {
    if (node.body) {
        const name = getNamesForFunctionLikeDeclaration(node);
        const descriptor = createDescriptor(name, node, source);
        dest.push(descriptor);
    }
}
function createDescriptor(name, range, source) {
    const { pos, end } = range;
    const { line: startLine, character: startColumn } = source.getLineAndCharacterOfPosition(pos);
    const { line: endLine, character: endColumn } = source.getLineAndCharacterOfPosition(end);
    return new NamedFunctionRange(name, { line: startLine, column: startColumn }, { line: endLine, column: endColumn });
}
function getNamesForFunctionLikeDeclaration(func) {
    let name = 'anonymousFunction';
    const nameNode = func.name;
    if (nameNode) {
        // named function, property name, identifier, string, computed property
        /**
         * function foo() {}   <--
         * class Sample {
         *   constructor() { }   NOT this one
         *   bar() { }    <--
         *   get baz() { }    <--
         *   set frob() { }    <--
         *   [Symbol.toString]()    <--
         * }
         */
        name = getNameOfNameNode(nameNode, func, name);
    }
    else if (tsc.isConstructorDeclaration(func)) {
        /**
         * class Sample {
         *   constructor() { }   <--
         * }
         */
        // (constructor for class Foo)
        const classDefinition = func.parent;
        if (tsc.isClassDeclaration(classDefinition)) {
            let className = 'anonymousClass';
            if (classDefinition.name) {
                className = classDefinition.name.text;
            }
            name = `constructorCall:, ${className}`;
        }
    }
    else {
        /**
         * const x = function() { }
         * const y = () => { }
         * const z = {
         *  frob: function() { },
         *  florbo: () => { },
         * }
         *
         * doSomething(function() { })
         * doSomething(() => { })
         */
        if (tsc.isFunctionExpression(func) || tsc.isArrowFunction(func)) {
            let parent = func.parent;
            // e.g., ( () => { } )
            if (tsc.isParenthesizedExpression(parent)) {
                parent = parent.parent;
            }
            if (tsc.isVariableDeclaration(parent) || tsc.isPropertyAssignment(parent) || tsc.isPropertyDeclaration(parent)) {
                if (parent.name && tsc.isIdentifier(parent.name)) {
                    name = getNameOfNameNode(parent.name, func, name);
                }
            }
            else if (tsc.isBinaryExpression(parent) && parent.operatorToken.kind === tsc.SyntaxKind.EqualsToken) {
                if (tsc.isPropertyAccessExpression(parent.left) || tsc.isElementAccessExpression(parent.left)) {
                    name = recursivelyGetPropertyAccessName(parent.left);
                }
                else if (tsc.isIdentifier(parent.left) || tsc.isStringLiteral(parent.left) || tsc.isNumericLiteral(parent.left)) {
                    name = parent.left.text;
                }
                // else unknown
            }
            else if (tsc.isCallOrNewExpression(func.parent) || tsc.isDecorator(func.parent)) {
                let parentExpressionName = recursivelyGetPropertyAccessName(func.parent.expression);
                if (tsc.isNewExpression(func.parent)) {
                    // Localization is not required: this is a programming expression ("new Foo")
                    parentExpressionName = `new ${parentExpressionName}`;
                }
                name = `anonymousCallbackTo: ${parentExpressionName}`;
            }
        }
    }
    return name;
}
function recursivelyGetPropertyAccessName(expression) {
    if (tsc.isPropertyAccessExpression(expression)) {
        return `${recursivelyGetPropertyAccessName(expression.expression)}.${expression.name.text}`;
    }
    if (tsc.isElementAccessExpression(expression)) {
        return `${recursivelyGetPropertyAccessName(expression.expression)}[${expression.argumentExpression}]`;
    }
    if (tsc.isCallExpression(expression)) {
        return expression.getText();
    }
    if (tsc.isIdentifier(expression) || tsc.isStringLiteral(expression) || tsc.isNumericLiteral(expression)) {
        return expression.text;
    }
    return 'computedProperty';
}
function getNameOfNameNode(nameNode, declaringNode, fallback) {
    let nameText = fallback;
    switch (nameNode.kind) {
        case tsc.SyntaxKind.ComputedPropertyName:
            if (tsc.isIdentifier(nameNode.expression)) {
                nameText = `[${nameNode.expression.text}]`;
            }
            else if (tsc.isStringLiteral(nameNode.expression) || tsc.isNumericLiteral(nameNode.expression)) {
                nameText = `[${nameNode.expression.text}]`;
            }
            else {
                nameText = 'computedProperty';
            }
            break;
        case tsc.SyntaxKind.StringLiteral:
        case tsc.SyntaxKind.NumericLiteral:
        case tsc.SyntaxKind.Identifier:
        case tsc.SyntaxKind.PrivateIdentifier:
            nameText = nameNode.text;
            break;
    }
    if (tsc.isGetAccessor(declaringNode) || tsc.isGetAccessorDeclaration(declaringNode)) {
        nameText = `get ${nameText}`;
    }
    else if (tsc.isSetAccessor(declaringNode) || tsc.isSetAccessor(declaringNode)) {
        nameText = `set ${nameText}`;
    }
    if (declaringNode.parent && tsc.isClassDeclaration(declaringNode.parent)) {
        let className = 'anonymousClass)';
        if (declaringNode.parent.name) {
            className = declaringNode.parent.name.text;
        }
        nameText = `${className}.${nameText}`;
    }
    return nameText;
}
function isSourceMapScriptFile(resouce) {
    if (resouce && resouce.url && resouce.type === 'sm-script') {
        const url = resouce.url.toLowerCase();
        return url?.endsWith('.js') || url?.endsWith('.ts') || url?.endsWith('.jsx') || url?.endsWith('.tsx') ||
            url?.endsWith('.mjs') || url?.endsWith('.cjs');
    }
    return false;
}
// @ts-ignore
chrome.devtools.inspectedWindow.onResourceAdded.addListener(async (resource) => {
    if (isSourceMapScriptFile(resource)) {
        const scriptResource = await new Promise(r => resource.getContent((content, encoding) => r({ url: resource.url, content, encoding })));
        if (scriptResource.content) {
            const ranges = parse(resource.url, scriptResource.content);
            try {
                await (resource).setFunctionRangesForScript(ranges);
            }
            catch (e) {
                return e;
            }
        }
    }
});
//# sourceMappingURL=DevToolsPlugin.js.map