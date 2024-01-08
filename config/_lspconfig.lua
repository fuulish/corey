require('lspconfig.configs').corey = {
  default_config = {
    cmd = {"corey"},
    filetypes = {'c', 'cpp', 'rust'},
    root_dir = lspconfig.util.root_pattern(".review.yml"),
    settings = {},
  };
}

require'lspconfig'.corey.setup{
	autostart = false
}

