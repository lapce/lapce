I'm a Vim fan. After years of using it, I started to customized it like everybody else. I thought I had something that's worked quite well, until I hit one problem. My linter sometimes took seconds to check the code, and it will freeze Vim, because Vim didn't have asynchronous support. I looked around; I found NeoVim and problem solved. Happy days. 

But there was always this one thing I can't solve. I want to make Vim pretty. I tried all kinds of themes. Tried all kinds of status bar plugins with icons in it. File explorers with file icons in it. But no, I also wanted a 1px vertical split bar, not the thick colored one or the dashed line drawn with pipe characters. After hours and hours of search, it occurred to me that it's impossible to do. Again NeoVim was the savior. It introduced external UI. The idea was that NeoVim splits the UI and the backend. The backend emits events to the UI and the UI draws them. I couldn't wait to try it out with writing a UI in Electron, with the hope that I can solve my vertical split bar dream. But I didn't, because it emits the drawing events treating it as a whole canvas, and I don't get the boundary of each splits. I started to hack NeoVim code. I made it to emit split sizes and positions, and I finally can draw the bars in the UI. With joy, I also looked at emits the command modes to the UI, so I can put the command in the middle of the window. And then echo messages, status line etc. I pushed a PR for the hacky solution(https://github.com/neovim/neovim/pull/5686). Then I hit performance issue in Electron because Javascript was simply not fast enough to deserialize the drawing events from NeoVim. So I wrote another UI in Go with Qt binding(https://github.com/dzhou121/gonvim). 

I wanted to external more and more components in NeoVim but I found it harder and harder, and I spotted Xi-Editor. I started to write a UI for it straightaway. Creating a code editor with Vim editing experience was the plan. There were things missing that I had to add in Xi. And working on it was so much easier, because Xi was built to be the UI/backend architecture without the heritage(burden) of (Neo)Vim. 

Then one day I experienced VSCode's remote development feature, and it felt so "local". I wanted to add the feature to my own code editor. Then I released it can't be done with NeoVim or Xi's UI/backend architecture. The reason was that the backends were the editing engine, so when you put the backend to a remote machine, every keyboard input needs to be sent to it, and the backend emits the drawing events to you, which will include the network latency in everything you type. That simply wouldn't work. The editing logic has be tightly bound with the UI to give the best editing experience. 

So the new architecture I came up with was like this: 

          UI
Reads file from Proxy
Handle keyboard/mouse events and do the edits on the file buffer
Send the file editing delta to the proxy to keep the file in sync
          |
	  |
	  |
	 Proxy
Receive save event from UI and flush the file buffer to disk
proxy the events between UI and the plugins
          |
	  |
	Plugin
Communicate with UI through proxy

UI sits locally. Proxy and Plugin will be in the remote box when doing remote development. With this architecture, I can make sure the editing experience is always the best, with other considerations like syntax highlighting is done in a different thread so nothing blocks the main thread at any time. I finally had a lighting-fast and powerful code editor. Lapce. (which can be beautiful as well)   