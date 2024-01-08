call plug#begin(has('nvim') ? stdpath('data') . '/plugged' : '~/.vim/plugged')
Plug 'neovim/nvim-lspconfig'
call plug#end()

nnoremap <leader>er :LspStart ghapi<CR>
source $HOME/.config/nvim/_lspconfig.lua
